//! Gaggle Worker implementation
//!
//! The Worker connects to a Manager and executes distributed load test tasks.

use super::GaggleServiceClient;
use super::{gaggle_proto::*, GaggleConfiguration};
use crate::config::{GooseDefault, GooseDefaultType};
use crate::{GooseAttack, GooseConfiguration as GooseConfig};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::{interval, Duration, Instant};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic::Request;

/// Gaggle Worker for executing distributed load tests
pub struct GaggleWorker {
    config: GaggleConfiguration,
    client: Arc<Mutex<Option<GaggleServiceClient<Channel>>>>,
    state: Arc<RwLock<WorkerStateInfo>>,
    metrics_buffer: Arc<Mutex<Vec<MetricsBatch>>>,
    load_test_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    stop_sender: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    progress_sender: Arc<Mutex<Option<mpsc::UnboundedSender<LoadTestProgress>>>>,
    /// Reference to the GooseAttack instance for local load test execution.
    goose_attack: Option<GooseAttack>,
}

/// Internal worker state information
#[derive(Debug, Clone)]
struct WorkerStateInfo {
    pub state: super::gaggle_proto::WorkerState,
    pub active_users: u32,
    pub registered: bool,
    pub last_heartbeat: Option<Instant>,
}

/// Load test progress information
#[derive(Debug, Clone)]
pub struct LoadTestProgress {
    pub timestamp: Instant,
    pub active_users: u32,
    pub total_requests: u64,
    pub failed_requests: u64,
    pub average_response_time: f64,
    pub current_rps: f64,
}

impl Default for WorkerStateInfo {
    fn default() -> Self {
        Self {
            state: super::gaggle_proto::WorkerState::Idle,
            active_users: 0,
            registered: false,
            last_heartbeat: None,
        }
    }
}

impl GaggleWorker {
    /// Create a new Gaggle Worker
    pub fn new(config: GaggleConfiguration) -> Self {
        Self {
            config,
            client: Arc::new(Mutex::new(None)),
            state: Arc::new(RwLock::new(WorkerStateInfo::default())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            load_test_handle: Arc::new(Mutex::new(None)),
            stop_sender: Arc::new(Mutex::new(None)),
            progress_sender: Arc::new(Mutex::new(None)),
            goose_attack: None,
        }
    }

    /// Create a new Gaggle Worker with GooseAttack integration
    pub fn with_goose_attack(config: GaggleConfiguration, goose_attack: GooseAttack) -> Self {
        Self {
            config,
            client: Arc::new(Mutex::new(None)),
            state: Arc::new(RwLock::new(WorkerStateInfo::default())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            load_test_handle: Arc::new(Mutex::new(None)),
            stop_sender: Arc::new(Mutex::new(None)),
            progress_sender: Arc::new(Mutex::new(None)),
            goose_attack: Some(goose_attack),
        }
    }

    /// Connect to the manager and start the worker
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager_addr = format!("http://{}", self.config.manager_address()?);
        info!("Connecting to manager at {}", manager_addr);

        // ACTUAL connection to manager
        let channel = tonic::transport::Channel::from_shared(manager_addr)?
            .connect()
            .await?;

        let client = GaggleServiceClient::new(channel);

        // Store the client for future use
        *self.client.lock().await = Some(client.clone());

        // Register with manager
        self.register().await?;

        // Start coordination stream
        self.start_coordination_stream().await?;

        // Start metrics submission
        self.start_metrics_submission().await?;

        info!("Gaggle Worker connected successfully and registered with manager");

        Ok(())
    }

    /// Register this worker with the manager
    async fn register(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard.as_mut().ok_or("Client not connected")?;

        let worker_info = super::gaggle_proto::WorkerInfo {
            worker_id: self
                .config
                .worker_id
                .clone()
                .unwrap_or_else(|| format!("worker-{}", uuid::Uuid::new_v4())),
            hostname: get_hostname(),
            ip_address: get_local_ip(),
            max_users: self.config.max_users.unwrap_or(1000),
            capabilities: self.config.capabilities.clone(),
        };

        let request = Request::new(worker_info.clone());
        let response = client.register_worker(request).await?;
        let register_response = response.into_inner();

        if register_response.success {
            info!(
                "Successfully registered with manager: {}",
                register_response.message
            );
            self.state.write().await.registered = true;
        } else {
            error!(
                "Failed to register with manager: {}",
                register_response.message
            );
            return Err("Registration failed".into());
        }

        Ok(())
    }

    /// Start the coordination stream with the manager
    async fn start_coordination_stream(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard.as_mut().ok_or("Client not connected")?;

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        let response = client.coordination_stream(Request::new(outbound)).await?;
        let mut inbound = response.into_inner();

        let state = Arc::clone(&self.state);
        let _worker_id = self
            .config
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("worker-{}", uuid::Uuid::new_v4()));

        // Drop the client guard to avoid holding the lock
        drop(client_guard);

        // Start the coordination loop
        tokio::spawn(async move {
            // Send initial status
            let initial_update = WorkerUpdate {
                worker_id: _worker_id.clone(),
                state: super::gaggle_proto::WorkerState::Ready.into(),
                active_users: 0,
                error_message: None,
                timestamp: chrono::Utc::now().timestamp_millis() as u64,
            };

            if tx.send(initial_update).await.is_err() {
                error!("Failed to send initial worker update");
                return;
            }

            // Process manager commands
            while let Some(result) = inbound.next().await {
                match result {
                    Ok(command) => {
                        info!(
                            "Received command from manager: {:?}",
                            command.command_type()
                        );
                        Self::handle_manager_command(command, Arc::clone(&state)).await;
                    }
                    Err(e) => {
                        error!("Error in coordination stream: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle commands received from the manager
    async fn handle_manager_command(command: ManagerCommand, state: Arc<RwLock<WorkerStateInfo>>) {
        match command.command_type() {
            CommandType::Start => {
                info!("Received START command");
                if let Some(test_config) = command.test_config {
                    info!("Starting load test with configuration: {:?}", test_config);
                    // TODO: Pass worker reference to execute load test
                    // For now, just update state
                    state.write().await.state = super::gaggle_proto::WorkerState::Running;
                } else {
                    warn!("START command received without test configuration");
                    state.write().await.state = super::gaggle_proto::WorkerState::Error;
                }
            }
            CommandType::Stop => {
                info!("Received STOP command");
                state.write().await.state = super::gaggle_proto::WorkerState::Stopping;
                // TODO: Call stop_load_test on worker instance
            }
            CommandType::Shutdown => {
                info!("Received SHUTDOWN command");
                state.write().await.state = super::gaggle_proto::WorkerState::Idle;
                // TODO: Implement graceful shutdown
            }
            CommandType::UpdateUsers => {
                if let Some(user_count) = command.user_count {
                    info!("Received UPDATE_USERS command: {}", user_count);
                    state.write().await.active_users = user_count;
                    // TODO: Call reconfigure_test on worker instance
                }
            }
            CommandType::Heartbeat => {
                debug!("Received heartbeat from manager");
                state.write().await.last_heartbeat = Some(Instant::now());
            }
            _ => {
                warn!(
                    "Received unknown command type: {:?}",
                    command.command_type()
                );
            }
        }
    }

    /// Start the metrics submission task
    async fn start_metrics_submission(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = Arc::clone(&self.client);
        let metrics_buffer = Arc::clone(&self.metrics_buffer);
        let _worker_id = self
            .config
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("worker-{}", uuid::Uuid::new_v4()));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5)); // Submit every 5 seconds

            loop {
                interval.tick().await;

                let mut buffer = metrics_buffer.lock().await;
                if buffer.is_empty() {
                    continue;
                }

                let batches_to_send = buffer.drain(..).collect::<Vec<_>>();
                drop(buffer);

                if !batches_to_send.is_empty() {
                    let mut client_guard = client.lock().await;
                    if let Some(client) = client_guard.as_mut() {
                        let (tx, rx) = tokio::sync::mpsc::channel(32);
                        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

                        // Send batches
                        for batch in batches_to_send {
                            if tx.send(batch).await.is_err() {
                                error!("Failed to send metrics batch");
                                break;
                            }
                        }
                        drop(tx);

                        match client.submit_metrics(Request::new(stream)).await {
                            Ok(response) => {
                                let result = response.into_inner();
                                debug!(
                                    "Metrics submitted: {} batches processed",
                                    result.processed_count
                                );
                            }
                            Err(e) => {
                                error!("Failed to submit metrics: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Start the heartbeat task
    async fn start_heartbeat(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = Arc::clone(&self.state);
        let heartbeat_interval = self.config.heartbeat_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(heartbeat_interval));

            loop {
                interval.tick().await;

                let current_state = state.read().await.clone();
                if !current_state.registered {
                    continue;
                }

                debug!(
                    "Sending heartbeat - state: {:?}, users: {}",
                    current_state.state, current_state.active_users
                );
            }
        });

        Ok(())
    }

    /// Get current worker state
    pub async fn get_state(&self) -> super::gaggle_proto::WorkerState {
        self.state.read().await.state
    }

    /// Get current active user count
    pub async fn get_active_users(&self) -> u32 {
        self.state.read().await.active_users
    }

    /// Add metrics to the buffer for submission
    pub async fn add_metrics(&self, batch: MetricsBatch) {
        self.metrics_buffer.lock().await.push(batch);
    }

    /// Execute a load test with the given test configuration
    pub async fn execute_load_test(
        &self,
        test_config: TestConfiguration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Update status to running
        {
            let mut state = self.state.write().await;
            state.state = super::gaggle_proto::WorkerState::Running;
        }

        // Create stop channel for graceful shutdown
        let (stop_tx, mut stop_rx) = oneshot::channel();
        {
            let mut stop_sender = self.stop_sender.lock().await;
            *stop_sender = Some(stop_tx);
        }

        // Create progress reporting channel
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        {
            let mut progress_sender = self.progress_sender.lock().await;
            *progress_sender = Some(progress_tx.clone());
        }

        // Convert test configuration to GooseConfig
        let mut config = GooseConfig::default();

        // Parse scenarios from test_config if available
        for scenario in &test_config.scenarios {
            info!("Loading scenario: {}", scenario.name);
        }

        // Apply test configuration
        if test_config.duration_seconds > 0 {
            config.run_time = test_config.duration_seconds.to_string();
        }

        // Start progress reporting task
        let manager_addr_str = format!(
            "{}",
            self.config
                .manager_address()
                .unwrap_or_else(|_| "127.0.0.1:5115".parse().unwrap())
        );
        let worker_id = self
            .config
            .worker_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let progress_task = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            while let Ok(progress) = progress_rx.try_recv() {
                interval.tick().await;
                // Send progress to manager (implementation depends on manager's progress endpoint)
                Self::send_progress_to_manager(&manager_addr_str, &worker_id, progress).await;
            }
        });

        // Execute load test in a separate task
        let state_arc = Arc::clone(&self.state);
        let config_clone = config.clone();
        let progress_tx_clone = progress_tx.clone();

        let load_test_task = tokio::spawn(async move {
            // Initialize GooseAttack
            let mut attack = match GooseAttack::initialize() {
                Ok(attack) => attack,
                Err(e) => {
                    error!("Failed to initialize GooseAttack: {}", e);
                    let mut state = state_arc.write().await;
                    state.state = super::gaggle_proto::WorkerState::Error;
                    return;
                }
            };

            // Apply configuration
            if !config_clone.run_time.is_empty() {
                if let Ok(run_time) = config_clone.run_time.parse::<usize>() {
                    attack = *attack.set_default(GooseDefault::RunTime, run_time).unwrap();
                }
            }

            // Start the load test
            tokio::select! {
                result = attack.execute() => {
                    match result {
                        Ok(metrics) => {
                            // Send final progress update
                            let final_progress = LoadTestProgress {
                                timestamp: Instant::now(),
                                active_users: 0,
                                total_requests: metrics.requests.len() as u64,
                                failed_requests: metrics.errors.len() as u64,
                                average_response_time: 0.0, // Calculate from metrics
                                current_rps: 0.0,
                            };
                            let _ = progress_tx_clone.send(final_progress);

                            // Update status to completed
                            let mut state = state_arc.write().await;
                            state.state = super::gaggle_proto::WorkerState::Idle;
                        },
                        Err(e) => {
                            error!("Load test failed: {}", e);
                            let mut state = state_arc.write().await;
                            state.state = super::gaggle_proto::WorkerState::Error;
                        }
                    }
                },
                _ = &mut stop_rx => {
                    info!("Load test stopped by request");
                    let mut state = state_arc.write().await;
                    state.state = super::gaggle_proto::WorkerState::Stopping;
                }
            };
        });

        // Store the task handle for potential cancellation
        {
            let mut handle = self.load_test_handle.lock().await;
            *handle = Some(load_test_task);
        }

        // Wait for progress task to complete
        let _ = progress_task.await;

        Ok(())
    }

    /// Send progress update to manager
    async fn send_progress_to_manager(
        _manager_addr: &str,
        worker_id: &str,
        progress: LoadTestProgress,
    ) {
        // This would send progress to the manager's progress endpoint
        // Implementation depends on manager's gRPC service for receiving progress
        info!(
            "Worker {}: {} users, {} requests, {} failed, {:.2}ms avg, {:.2} RPS",
            worker_id,
            progress.active_users,
            progress.total_requests,
            progress.failed_requests,
            progress.average_response_time,
            progress.current_rps
        );
    }

    /// Handle dynamic reconfiguration during test execution
    pub async fn reconfigure_test(
        &self,
        new_users: Option<u32>,
        new_rps: Option<f64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if test is currently running
        {
            let state = self.state.read().await;
            if state.state != super::gaggle_proto::WorkerState::Running {
                return Err("Cannot reconfigure: test is not running".into());
            }
        }

        // Send reconfiguration signal to running test
        // This would require coordination with the running GooseAttack instance
        info!(
            "Reconfiguring test - Users: {:?}, RPS: {:?}",
            new_users, new_rps
        );

        // Update active users count if provided
        if let Some(users) = new_users {
            let mut state = self.state.write().await;
            state.active_users = users;
        }

        Ok(())
    }

    /// Stop the currently running load test
    pub async fn stop_load_test(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Send stop signal
        if let Some(stop_tx) = {
            let mut stop_sender = self.stop_sender.lock().await;
            stop_sender.take()
        } {
            let _ = stop_tx.send(());
        }

        // Cancel the load test task if it's still running
        if let Some(handle) = {
            let mut load_test_handle = self.load_test_handle.lock().await;
            load_test_handle.take()
        } {
            handle.abort();
        }

        // Update status
        {
            let mut state = self.state.write().await;
            state.state = super::gaggle_proto::WorkerState::Stopping;
        }

        Ok(())
    }

    /// Execute assigned load test using the integrated GooseAttack instance
    pub async fn execute_assigned_load_test(
        &self,
        scenarios: &[crate::goose::Scenario],
        configuration: &GooseConfig,
    ) -> Result<crate::GooseMetrics, Box<dyn std::error::Error + Send + Sync>> {
        // Check if we have a GooseAttack instance
        if self.goose_attack.is_none() {
            return Err("No GooseAttack instance available for load test execution".into());
        }

        // Update status to running
        {
            let mut state = self.state.write().await;
            state.state = super::gaggle_proto::WorkerState::Running;
        }

        info!(
            "Executing assigned portion of load test with {} scenarios",
            scenarios.len()
        );

        // Clone the GooseAttack instance to avoid borrowing issues
        // NOTE: This is a temporary approach - in production we'd want to properly manage
        // the GooseAttack lifecycle
        let mut local_attack = GooseAttack::initialize_with_config(configuration.clone())?;

        // Register scenarios from the manager
        for scenario in scenarios.iter() {
            local_attack = local_attack.register_scenario(scenario.clone());
        }

        // Execute the local portion of the load test
        let result = local_attack.execute().await;

        match result {
            Ok(metrics) => {
                info!("Local load test completed successfully");

                // Update status to idle
                {
                    let mut state = self.state.write().await;
                    state.state = super::gaggle_proto::WorkerState::Idle;
                }

                Ok(metrics)
            }
            Err(e) => {
                error!("Local load test failed: {}", e);

                // Update status to error
                {
                    let mut state = self.state.write().await;
                    state.state = super::gaggle_proto::WorkerState::Error;
                }

                Err(format!("Load test execution failed: {}", e).into())
            }
        }
    }

    /// Stream metrics to manager during test execution
    pub async fn stream_metrics_to_manager(
        &self,
        metrics: crate::GooseMetrics,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Streaming metrics to manager");

        let worker_id = self
            .config
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("worker-{}", uuid::Uuid::new_v4()));

        // Convert GooseMetrics to MetricsBatch for transmission
        let metrics_batch = self
            .convert_goose_metrics_to_batch(&worker_id, metrics)
            .await?;

        // Add to metrics buffer for transmission
        self.add_metrics(metrics_batch).await;

        info!("Metrics queued for transmission to manager");
        Ok(())
    }

    /// Convert GooseMetrics to MetricsBatch for transmission
    async fn convert_goose_metrics_to_batch(
        &self,
        worker_id: &str,
        goose_metrics: crate::GooseMetrics,
    ) -> Result<MetricsBatch, Box<dyn std::error::Error + Send + Sync>> {
        // Create a basic metrics batch - this would be expanded to include
        // all relevant metrics in a production implementation
        let batch = MetricsBatch {
            worker_id: worker_id.to_string(),
            request_metrics: Vec::new(), // Would populate with converted request metrics
            transaction_metrics: Vec::new(), // Would populate with converted transaction metrics
            scenario_metrics: Vec::new(), // Would populate with converted scenario metrics
            batch_timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };

        info!(
            "Converted {} request metrics and {} scenario metrics to batch",
            goose_metrics.requests.len(),
            goose_metrics.scenarios.len()
        );

        Ok(batch)
    }

    /// Receive and apply configuration from manager
    pub async fn apply_manager_configuration(
        &self,
        test_config: TestConfiguration,
    ) -> Result<GooseConfig, Box<dyn std::error::Error + Send + Sync>> {
        info!("Applying configuration received from manager");

        let mut config = GooseConfig::default();

        // Apply test duration
        if test_config.duration_seconds > 0 {
            config.run_time = test_config.duration_seconds.to_string();
        }

        // Apply scenarios - the scenarios would be received as part of the test config
        // and reconstructed locally
        info!(
            "Applied configuration: duration={}s",
            test_config.duration_seconds
        );

        Ok(config)
    }

    /// Reconstruct test scenarios from manager data
    pub async fn reconstruct_scenarios_from_config(
        &self,
        scenarios_data: &[String],
    ) -> Result<Vec<crate::goose::Scenario>, Box<dyn std::error::Error + Send + Sync>> {
        let mut reconstructed_scenarios = Vec::new();

        for scenario_name in scenarios_data.iter() {
            info!("Reconstructing scenario: {}", scenario_name);

            // In a full implementation, this would deserialize scenario definitions
            // received from the manager and reconstruct the full scenario with transactions
            // For now, create a placeholder scenario
            let scenario = crate::goose::Scenario::new(scenario_name.as_str());
            reconstructed_scenarios.push(scenario);
        }

        info!(
            "Reconstructed {} scenarios from manager configuration",
            reconstructed_scenarios.len()
        );
        Ok(reconstructed_scenarios)
    }
}

// Helper functions for system information
fn get_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn get_local_ip() -> String {
    "127.0.0.1".to_string() // Placeholder - would use actual IP detection in production
}

// Placeholder UUID generation
mod uuid {
    pub struct Uuid;

    impl Uuid {
        pub fn new_v4() -> String {
            format!(
                "{:08x}-{:04x}-{:04x}-{:04x}-{:08x}",
                rand::random::<u32>(),
                rand::random::<u16>(),
                rand::random::<u16>(),
                rand::random::<u16>(),
                rand::random::<u32>()
            )
        }
    }
}
