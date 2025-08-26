//! Gaggle Worker implementation
//!
//! The Worker connects to a Manager and executes distributed load test tasks.

use super::{gaggle_proto::*, GaggleConfiguration};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration, Instant};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic::{Request, Streaming};

/// Gaggle Worker for executing distributed load tests
pub struct GaggleWorker {
    config: GaggleConfiguration,
    client: Arc<Mutex<Option<GaggleServiceClient<Channel>>>>,
    state: Arc<RwLock<WorkerStateInfo>>,
    metrics_buffer: Arc<Mutex<Vec<MetricsBatch>>>,
}

/// Internal worker state information
#[derive(Debug, Clone)]
struct WorkerStateInfo {
    pub state: super::gaggle_proto::WorkerState,
    pub active_users: u32,
    pub registered: bool,
    pub last_heartbeat: Option<Instant>,
}

impl Default for WorkerStateInfo {
    fn default() -> Self {
        Self {
            state: super::gaggle_proto::WorkerState::WorkerStateIdle,
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
        }
    }

    /// Connect to the manager and start the worker
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let manager_addr = format!("http://{}", self.config.manager_address()?);
        info!("Connecting to manager at {}", manager_addr);

        // Establish connection
        let channel = tonic::transport::Endpoint::new(manager_addr)?
            .timeout(Duration::from_secs(self.config.connection_timeout))
            .connect()
            .await?;

        let client = GaggleServiceClient::new(channel);
        *self.client.lock().await = Some(client);

        // Register with manager
        self.register().await?;

        // Start coordination stream
        let coordination_handle = self.start_coordination_stream();

        // Start metrics submission
        let metrics_handle = self.start_metrics_submission();

        // Start heartbeat
        let heartbeat_handle = self.start_heartbeat();

        // Wait for all tasks
        tokio::try_join!(coordination_handle, metrics_handle, heartbeat_handle)?;

        Ok(())
    }

    /// Register this worker with the manager
    async fn register(&self) -> Result<(), Box<dyn std::error::Error>> {
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
    async fn start_coordination_stream(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard.as_mut().ok_or("Client not connected")?;

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        let response = client.coordination_stream(Request::new(outbound)).await?;
        let mut inbound = response.into_inner();

        let state = Arc::clone(&self.state);
        let worker_id = self
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
                worker_id: worker_id.clone(),
                state: super::gaggle_proto::WorkerState::WorkerStateReady.into(),
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
            CommandType::CommandTypeStart => {
                info!("Received START command");
                state.write().await.state = super::gaggle_proto::WorkerState::WorkerStateRunning;
                // Implementation for starting load test will be added in later phases
            }
            CommandType::CommandTypeStop => {
                info!("Received STOP command");
                state.write().await.state = super::gaggle_proto::WorkerState::WorkerStateStopping;
                // Implementation for stopping load test will be added in later phases
            }
            CommandType::CommandTypeShutdown => {
                info!("Received SHUTDOWN command");
                state.write().await.state = super::gaggle_proto::WorkerState::WorkerStateIdle;
                // Implementation for shutdown will be added in later phases
            }
            CommandType::CommandTypeUpdateUsers => {
                if let Some(user_count) = command.user_count {
                    info!("Received UPDATE_USERS command: {}", user_count);
                    state.write().await.active_users = user_count;
                    // Implementation for user count update will be added in later phases
                }
            }
            CommandType::CommandTypeHeartbeat => {
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
    async fn start_metrics_submission(&self) -> Result<(), Box<dyn std::error::Error>> {
        let client = Arc::clone(&self.client);
        let metrics_buffer = Arc::clone(&self.metrics_buffer);
        let worker_id = self
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
    async fn start_heartbeat(&self) -> Result<(), Box<dyn std::error::Error>> {
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
