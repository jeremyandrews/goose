//! Gaggle Manager implementation
//!
//! The Manager coordinates multiple Workers in a distributed load test.

use super::{gaggle_proto::*, GaggleConfiguration};
use crate::gaggle::gaggle_service_server::{GaggleService, GaggleServiceServer};
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
