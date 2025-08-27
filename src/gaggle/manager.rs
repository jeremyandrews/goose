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

    /// Send command to all workers
    pub async fn broadcast_command(
        &self,
        command: ManagerCommand,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation will be expanded in later phases
        info!("Broadcasting command: {:?}", command.command_type());
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
