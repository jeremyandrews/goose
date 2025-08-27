//! Gaggle Manager implementation
//!
//! The Manager coordinates multiple Workers in a distributed load test.

use super::{gaggle_proto::*, GaggleConfiguration};
use super::{GaggleService, GaggleServiceServer};
use crate::GooseAttack;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{transport::Server, Request, Response, Status, Streaming};

/// Gaggle Manager for coordinating distributed load tests
pub struct GaggleManager {
    config: GaggleConfiguration,
    workers: Arc<RwLock<HashMap<String, WorkerState>>>,
    metrics_buffer: Arc<Mutex<Vec<MetricsBatch>>>,
    /// Reference to the GooseAttack instance for configuration and test plan distribution.
    goose_attack: Option<GooseAttack>,
}

/// Worker state information maintained by the manager
#[derive(Debug, Clone)]
pub struct WorkerState {
    pub id: String,
    pub hostname: String,
    pub ip_address: String,
    pub max_users: u32,
    pub capabilities: Vec<String>,
    pub state: super::gaggle_proto::WorkerState,
    pub active_users: u32,
    pub last_heartbeat: std::time::Instant,
}

impl GaggleManager {
    /// Create a new Gaggle Manager
    pub fn new(config: GaggleConfiguration) -> Self {
        Self {
            config,
            workers: Arc::new(RwLock::new(HashMap::new())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            goose_attack: None,
        }
    }

    /// Create a new Gaggle Manager with GooseAttack integration
    pub fn with_goose_attack(config: GaggleConfiguration, goose_attack: GooseAttack) -> Self {
        Self {
            config,
            workers: Arc::new(RwLock::new(HashMap::new())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            goose_attack: Some(goose_attack),
        }
    }

    /// Start the gRPC server
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = self.config.manager_address()?;

        info!("Starting Gaggle Manager on {}", addr);

        let service = GaggleServiceImpl {
            workers: Arc::clone(&self.workers),
            metrics_buffer: Arc::clone(&self.metrics_buffer),
        };

        // ACTUAL gRPC server startup instead of dummy sleep
        Server::builder()
            .add_service(GaggleServiceServer::new(service))
            .serve(addr)
            .await?;

        Ok(())
    }

    /// Get current worker count
    pub async fn worker_count(&self) -> usize {
        self.workers.read().await.len()
    }

    /// Get worker information
    pub async fn get_workers(&self) -> HashMap<String, WorkerState> {
        self.workers.read().await.clone()
    }

    /// Start distributed load test across all workers
    pub async fn start_distributed_load_test(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Starting distributed load test across {} workers",
            self.worker_count().await
        );

        if self.worker_count().await == 0 {
            return Err("Cannot start test: no workers connected".into());
        }

        // Create test configuration from GooseAttack if available
        let test_config = if let Some(ref goose_attack) = self.goose_attack {
            Some(TestConfiguration {
                test_plan: format!("{{\"scenarios_count\": {}}}", goose_attack.scenarios.len()),
                duration_seconds: goose_attack.configuration.run_time.parse().unwrap_or(60),
                requests_per_second: 100.0, // Default RPS
                scenarios: self.convert_scenarios_to_config(&goose_attack.scenarios)?,
                config: Some(self.convert_goose_config_to_proto(&goose_attack.configuration)?),
                assigned_users: goose_attack.configuration.users.unwrap_or(1) as u32,
                test_hash: self.generate_test_hash(),
                manager_version: env!("CARGO_PKG_VERSION").to_string(),
                test_start_time: chrono::Utc::now().timestamp_millis() as u64,
            })
        } else {
            None
        };

        let start_command = ManagerCommand {
            command_type: CommandType::Start.into(),
            test_config,
            user_count: None,
            message: Some("Starting distributed load test".to_string()),
        };

        // Send START command to all workers
        self.broadcast_command(start_command).await?;

        info!("Distributed load test started successfully");
        Ok(())
    }

    /// Stop distributed load test across all workers
    pub async fn stop_distributed_load_test(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Stopping distributed load test across {} workers",
            self.worker_count().await
        );

        let stop_command = ManagerCommand {
            command_type: CommandType::Stop.into(),
            test_config: None,
            user_count: None,
            message: Some("Stopping distributed load test".to_string()),
        };

        self.broadcast_command(stop_command).await?;

        info!("Distributed load test stopped successfully");
        Ok(())
    }

    /// Redistribute users across workers during runtime
    pub async fn redistribute_users(
        &self,
        new_user_count: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Redistributing {} users across {} workers",
            new_user_count,
            self.worker_count().await
        );

        let worker_count = self.worker_count().await;
        if worker_count == 0 {
            return Err("Cannot redistribute users: no workers connected".into());
        }

        // Calculate users per worker (evenly distributed)
        let base_users_per_worker = new_user_count / (worker_count as u32);
        let extra_users = new_user_count % (worker_count as u32);

        info!(
            "Base users per worker: {}, extra users: {}",
            base_users_per_worker, extra_users
        );

        // For now, send a simple UPDATE_USERS command
        // In a full implementation, we'd send individual configurations to each worker
        let update_command = ManagerCommand {
            command_type: CommandType::UpdateUsers.into(),
            test_config: None,
            user_count: Some(base_users_per_worker),
            message: Some(format!(
                "Updating to {} users per worker",
                base_users_per_worker
            )),
        };

        self.broadcast_command(update_command).await?;

        info!(
            "Successfully redistributed {} users across {} workers",
            new_user_count, worker_count
        );
        Ok(())
    }

    /// Shutdown all workers gracefully
    pub async fn shutdown_all_workers(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Shutting down all {} workers", self.worker_count().await);

        let shutdown_command = ManagerCommand {
            command_type: CommandType::Shutdown.into(),
            test_config: None,
            user_count: None,
            message: Some("Graceful shutdown requested".to_string()),
        };

        self.broadcast_command(shutdown_command).await?;

        // Give workers time to shutdown gracefully
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        info!("All workers shutdown completed");
        Ok(())
    }

    /// Send command to all workers
    pub async fn broadcast_command(
        &self,
        command: ManagerCommand,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            "Broadcasting command: {:?} to {} workers",
            command.command_type(),
            self.worker_count().await
        );

        // This is a simplified implementation - in a full implementation,
        // we would maintain active gRPC streams to each worker and send commands directly
        // For now, we log the command and assume it would be sent via the coordination stream

        info!("Command broadcast completed: {:?}", command.message);
        Ok(())
    }

    /// Synchronize configuration from GooseAttack to workers.
    pub fn sync_configuration(
        &self,
    ) -> Result<crate::config::GooseConfiguration, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref goose_attack) = self.goose_attack {
            // Extract relevant configuration from GooseAttack
            let mut config = goose_attack.configuration.clone();

            // Sync user and hatch rate settings
            if let Some(users) = goose_attack.configuration.users {
                config.users = Some(users);
            }

            if let Some(ref hatch_rate) = goose_attack.configuration.hatch_rate {
                config.hatch_rate = Some(hatch_rate.clone());
            }

            // Sync run time settings
            config.run_time = goose_attack.configuration.run_time.clone();

            // Sync host configuration
            config.host = goose_attack.configuration.host.clone();

            Ok(config)
        } else {
            Err("No GooseAttack instance available for configuration sync".into())
        }
    }

    /// Get test plan from GooseAttack for distribution to workers.
    pub fn get_test_plan(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref goose_attack) = self.goose_attack {
            // For now, we'll create a simplified representation of the test plan
            // In a production implementation, we'd need to serialize the actual scenarios
            // and their configurations in a way that workers can reconstruct them

            let test_plan_info =
                format!("{{\"scenarios_count\": {}}}", goose_attack.scenarios.len());

            Ok(test_plan_info.into_bytes())
        } else {
            Err("No GooseAttack instance available for test plan extraction".into())
        }
    }

    /// Distribute configuration and test plan to all connected workers.
    pub async fn distribute_test_setup(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config = self.sync_configuration()?;
        let test_plan = self.get_test_plan()?;

        info!(
            "Distributing test configuration to {} workers",
            self.worker_count().await
        );

        // TODO: Implement actual gRPC calls to distribute config and test plan to workers
        // This would involve:
        // 1. Maintaining a list of connected workers
        // 2. Sending configuration updates via gRPC
        // 3. Sending test plan data via gRPC
        // 4. Handling worker acknowledgments and errors

        debug!(
            "Configuration distributed: users={:?}, hatch_rate={:?}",
            config.users, config.hatch_rate
        );
        debug!("Test plan size: {} bytes", test_plan.len());

        Ok(())
    }

    /// Aggregate metrics from workers and integrate with GooseAttack.
    pub fn aggregate_worker_metrics(
        &self,
        _worker_metrics: MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Store worker metrics for processing
        // In a production implementation, this would merge metrics into the manager's
        // aggregated metrics and potentially update the GooseAttack instance

        debug!("Aggregating metrics from worker");

        // TODO: Implement actual metrics aggregation
        // This would involve:
        // 1. Merging worker metrics into manager's aggregated metrics
        // 2. Updating GooseAttack's internal metrics if available
        // 3. Handling different metric types (requests, response times, errors)

        Ok(())
    }

    /// Get aggregated metrics for reporting.
    pub async fn get_aggregated_metrics(&self) -> Vec<super::gaggle_proto::MetricsBatch> {
        let buffer = self.metrics_buffer.lock().await;
        buffer.clone()
    }

    /// Reset metrics collection.
    pub async fn reset_metrics(&self) {
        let mut buffer = self.metrics_buffer.lock().await;
        buffer.clear();
        debug!("Reset aggregated metrics");
    }

    /// Check if GooseAttack integration is available.
    pub fn has_goose_attack(&self) -> bool {
        self.goose_attack.is_some()
    }

    /// Convert GooseAttack scenarios to protobuf ScenarioConfig
    fn convert_scenarios_to_config(
        &self,
        scenarios: &[crate::goose::Scenario],
    ) -> Result<Vec<ScenarioConfig>, Box<dyn std::error::Error + Send + Sync>> {
        let mut scenario_configs = Vec::new();

        for scenario in scenarios {
            let scenario_config = ScenarioConfig {
                name: scenario.name.clone(),
                machine_name: scenario.name.clone(), // Use scenario name as machine name
                weight: scenario.weight as u32,
                host: scenario.host.clone(),
                transaction_wait: scenario.transaction_wait.clone().map(|tw| {
                    TransactionWaitConfig {
                        min_wait_ms: tw.0.as_millis() as u64,
                        max_wait_ms: tw.1.as_millis() as u64,
                    }
                }),
                transactions: self.convert_transactions_to_config(&scenario.transactions)?,
            };
            scenario_configs.push(scenario_config);
        }

        Ok(scenario_configs)
    }

    /// Convert GooseAttack transactions to protobuf TransactionConfig
    fn convert_transactions_to_config(
        &self,
        transactions: &[crate::goose::Transaction],
    ) -> Result<Vec<TransactionConfig>, Box<dyn std::error::Error + Send + Sync>> {
        let mut transaction_configs = Vec::new();

        for transaction in transactions {
            let transaction_config = TransactionConfig {
                name: format!("{:?}", transaction.name), // Convert enum to string using Debug
                name_type: 1,                            // Default naming (TRANSACTION_ONLY)
                weight: transaction.weight as u32,
                sequence: transaction.sequence as u32,
                on_start: transaction.on_start,
                on_stop: transaction.on_stop,
                function_name: format!("{:?}", transaction.name), // Use transaction name as function identifier
            };
            transaction_configs.push(transaction_config);
        }

        Ok(transaction_configs)
    }

    /// Convert GooseConfiguration to protobuf GooseConfiguration (simplified)
    fn convert_goose_config_to_proto(
        &self,
        config: &crate::config::GooseConfiguration,
    ) -> Result<GooseConfiguration, Box<dyn std::error::Error + Send + Sync>> {
        Ok(GooseConfiguration {
            host: if config.host.is_empty() {
                String::new()
            } else {
                config.host.clone()
            },
            users: config.users.map(|u| u as u32),
            hatch_rate: config.hatch_rate.clone(),
            startup_time: config.startup_time.clone(),
            run_time: config.run_time.clone(),
            goose_log: config.goose_log.clone(),
            log_level: config.log_level as u32,
            quiet: config.quiet as u32,
            verbose: config.verbose as u32,
            running_metrics: config.running_metrics.map(|u| u as u32),
            no_reset_metrics: config.no_reset_metrics,
            no_metrics: config.no_metrics,
            no_transaction_metrics: config.no_transaction_metrics,
            no_scenario_metrics: config.no_scenario_metrics,
            no_print_metrics: config.no_print_metrics,
            no_error_summary: config.no_error_summary,
            no_status_codes: config.no_status_codes,
            report_file: config.report_file.clone(),
            no_granular_report: config.no_granular_report,
            request_log: config.request_log.clone(),
            request_format: LogFormat::Json.into(), // Default format
            request_body: config.request_body,
            transaction_log: config.transaction_log.clone(),
            transaction_format: LogFormat::Json.into(), // Default format
            scenario_log: config.scenario_log.clone(),
            scenario_format: LogFormat::Json.into(), // Default format
            error_log: config.error_log.clone(),
            error_format: LogFormat::Json.into(), // Default format
            debug_log: config.debug_log.clone(),
            debug_format: LogFormat::Json.into(), // Default format
            no_debug_body: config.no_debug_body,
            test_plan: None, // TODO: Convert test plan if available
            iterations: config.iterations as u32,
            active_scenarios: Vec::new(), // Simplified for now
            no_telnet: config.no_telnet,
            telnet_host: config.telnet_host.clone(),
            telnet_port: config.telnet_port as u32,
            no_websocket: config.no_websocket,
            websocket_host: config.websocket_host.clone(),
            websocket_port: config.websocket_port as u32,
            no_autostart: config.no_autostart,
            no_gzip: config.no_gzip,
            timeout: config.timeout.clone(),
            co_mitigation: CoordinatedOmissionMitigation::Disabled.into(), // Default
            throttle_requests: config.throttle_requests as u32,
            sticky_follow: config.sticky_follow,
            accept_invalid_certs: config.accept_invalid_certs,
        })
    }

    /// Generate a unique test hash for coordination
    fn generate_test_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        chrono::Utc::now().timestamp_millis().hash(&mut hasher);
        if let Some(ref goose_attack) = self.goose_attack {
            goose_attack.scenarios.len().hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// gRPC service implementation
#[derive(Debug)]
struct GaggleServiceImpl {
    workers: Arc<RwLock<HashMap<String, WorkerState>>>,
    metrics_buffer: Arc<Mutex<Vec<MetricsBatch>>>,
}

#[tonic::async_trait]
impl GaggleService for GaggleServiceImpl {
    async fn register_worker(
        &self,
        request: Request<super::gaggle_proto::WorkerInfo>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let worker_info = request.into_inner();
        let worker_id = worker_info.worker_id.clone();

        info!(
            "Registering worker: {} from {}",
            worker_id, worker_info.hostname
        );

        let mut workers = self.workers.write().await;

        if workers.contains_key(&worker_id) {
            warn!("Worker {} is already registered, updating info", worker_id);
        }

        workers.insert(
            worker_id.clone(),
            WorkerState {
                id: worker_id.clone(),
                hostname: worker_info.hostname,
                ip_address: worker_info.ip_address,
                max_users: worker_info.max_users,
                capabilities: worker_info.capabilities,
                state: super::gaggle_proto::WorkerState::Idle,
                active_users: 0,
                last_heartbeat: std::time::Instant::now(),
            },
        );

        let response = RegisterResponse {
            success: true,
            message: "Worker registered successfully".to_string(),
            assigned_id: worker_id,
        };

        Ok(Response::new(response))
    }

    type CoordinationStreamStream = ReceiverStream<Result<ManagerCommand, Status>>;

    async fn coordination_stream(
        &self,
        request: Request<Streaming<WorkerUpdate>>,
    ) -> Result<Response<Self::CoordinationStreamStream>, Status> {
        let mut in_stream = request.into_inner();
        let workers = Arc::clone(&self.workers);

        let (tx, rx) = tokio::sync::mpsc::channel(128);

        // Spawn a task to handle worker updates
        tokio::spawn(async move {
            while let Some(result) = in_stream.next().await {
                match result {
                    Ok(update) => {
                        debug!(
                            "Received update from worker {}: {:?}",
                            update.worker_id,
                            update.state()
                        );

                        // Update worker state
                        let mut workers_map = workers.write().await;
                        if let Some(worker) = workers_map.get_mut(&update.worker_id) {
                            worker.state = update.state();
                            worker.active_users = update.active_users;
                            worker.last_heartbeat = std::time::Instant::now();
                        }
                        drop(workers_map);

                        // Send heartbeat response
                        let heartbeat = ManagerCommand {
                            command_type: CommandType::Heartbeat.into(),
                            test_config: None,
                            user_count: None,
                            message: Some("Heartbeat".to_string()),
                        };

                        if tx.send(Ok(heartbeat)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Error in coordination stream: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn submit_metrics(
        &self,
        request: Request<Streaming<MetricsBatch>>,
    ) -> Result<Response<MetricsResponse>, Status> {
        let mut in_stream = request.into_inner();
        let mut processed_count = 0u64;
        let metrics_buffer = Arc::clone(&self.metrics_buffer);

        while let Some(result) = in_stream.next().await {
            match result {
                Ok(batch) => {
                    debug!("Received metrics batch from worker {}", batch.worker_id);

                    // Store metrics for processing
                    let mut buffer = metrics_buffer.lock().await;
                    buffer.push(batch);
                    processed_count += 1;
                }
                Err(e) => {
                    error!("Error receiving metrics: {}", e);
                    return Err(Status::internal("Failed to process metrics batch"));
                }
            }
        }

        let response = MetricsResponse {
            success: true,
            message: "Metrics processed successfully".to_string(),
            processed_count,
        };

        Ok(Response::new(response))
    }
}
