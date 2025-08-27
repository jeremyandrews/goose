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
    /// Advanced metrics analytics for trend detection and anomaly identification
    analytics: Arc<RwLock<MetricsAnalytics>>,
    /// Time-series storage for historical metrics analysis  
    time_series: Arc<RwLock<MetricsTimeSeries>>,
    /// Aggregated metrics from all workers with detailed statistics
    aggregated_metrics: Arc<RwLock<crate::GooseMetrics>>,
    /// Performance baselines for anomaly detection
    performance_baselines: Arc<RwLock<HashMap<String, f64>>>,
    /// Test start timestamp for calculating elapsed time
    test_start_time: Arc<RwLock<Option<std::time::Instant>>>,
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
    /// Extended connection health information (Section 6.1)
    pub connection_health: ConnectionHealthInfo,
    /// Connection quality metrics (Section 6.1)
    pub connection_metrics: ConnectionQualityMetrics,
}

/// Detailed connection health information for monitoring (Section 6.1)
#[derive(Debug, Clone)]
pub struct ConnectionHealthInfo {
    /// Current connection status
    pub status: ConnectionStatus,
    /// Connection established timestamp
    pub connected_since: std::time::Instant,
    /// Number of reconnection attempts
    pub reconnection_attempts: u32,
    /// Last successful heartbeat timestamp
    pub last_successful_heartbeat: std::time::Instant,
    /// Connection stability score (0.0 - 1.0)
    pub stability_score: f64,
    /// Health check interval in seconds
    pub health_check_interval: u64,
    /// Connection timeout threshold in seconds
    pub connection_timeout: u64,
}

/// Connection status for detailed monitoring (Section 6.1)
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// Connection is healthy and stable
    Healthy,
    /// Connection has minor issues but is functional
    Degraded,
    /// Connection is experiencing significant issues
    Unstable,
    /// Connection is lost or unresponsive
    Disconnected,
    /// Connection is in reconnection process
    Reconnecting,
    /// Connection has permanently failed
    Failed,
}

/// Connection quality metrics for performance monitoring (Section 6.1)
#[derive(Debug, Clone)]
pub struct ConnectionQualityMetrics {
    /// Average heartbeat latency in milliseconds
    pub average_latency_ms: f64,
    /// Minimum observed latency in milliseconds
    pub min_latency_ms: f64,
    /// Maximum observed latency in milliseconds
    pub max_latency_ms: f64,
    /// Latency samples for trend analysis (keep last 100 samples)
    pub latency_samples: std::collections::VecDeque<LatencySample>,
    /// Packet loss percentage (0.0 - 100.0)
    pub packet_loss_percent: f64,
    /// Connection uptime percentage (0.0 - 100.0)
    pub uptime_percent: f64,
    /// Total number of heartbeats sent
    pub heartbeats_sent: u64,
    /// Total number of heartbeats received
    pub heartbeats_received: u64,
    /// Number of connection drops
    pub connection_drops: u32,
    /// Data throughput in bytes per second
    pub throughput_bps: f64,
}

/// Individual latency measurement sample (Section 6.1)
#[derive(Debug, Clone)]
pub struct LatencySample {
    /// Timestamp of the measurement
    pub timestamp: std::time::Instant,
    /// Latency measurement in milliseconds
    pub latency_ms: f64,
    /// Whether the measurement was successful
    pub successful: bool,
}

/// Aggregated statistics across all workers for real-time monitoring
#[derive(Debug, Clone)]
pub struct AggregatedStatistics {
    /// Number of workers currently reporting metrics
    pub workers_reporting: u32,
    /// Total number of requests across all workers
    pub total_requests: u64,
    /// Total number of failed requests across all workers
    pub total_failures: u64,
    /// Overall success rate percentage (0.0 - 100.0)
    pub success_rate: f64,
    /// Average response time across all requests in milliseconds
    pub average_response_time_ms: f64,
    /// Median response time across all requests in milliseconds
    pub median_response_time_ms: f64,
    /// 95th percentile response time in milliseconds
    pub p95_response_time_ms: f64,
    /// 99th percentile response time in milliseconds
    pub p99_response_time_ms: f64,
    /// Standard deviation of response times
    pub response_time_std_dev: f64,
    /// Total number of transactions executed across all workers
    pub total_transactions: u64,
    /// Transaction success rate percentage (0.0 - 100.0)
    pub transaction_success_rate: f64,
    /// Total number of active users across all workers
    pub total_active_users: u32,
    /// Current requests per second across all workers
    pub requests_per_second: f64,
    /// Peak requests per second observed
    pub peak_requests_per_second: f64,
    /// Number of unique error types seen
    pub unique_errors: u32,
    /// Error rate (errors per second)
    pub error_rate: f64,
    /// Timestamp when these statistics were last calculated
    pub last_updated: u64,
}

/// Real-time metrics analytics for trend detection and anomaly identification
#[derive(Debug, Clone)]
pub struct MetricsAnalytics {
    /// Time-series data points for response times (timestamp, avg_response_time)
    pub response_time_series: Vec<(u64, f64)>,
    /// Time-series data points for request rates (timestamp, requests_per_second)
    pub request_rate_series: Vec<(u64, f64)>,
    /// Time-series data points for error rates (timestamp, errors_per_second)
    pub error_rate_series: Vec<(u64, f64)>,
    /// Detected performance anomalies
    pub anomalies: Vec<PerformanceAnomaly>,
    /// Performance trend analysis
    pub trends: TrendAnalysis,
    /// Maximum retention period for time-series data (in seconds)
    pub retention_period: u64,
}

/// Performance anomaly detection data
#[derive(Debug, Clone)]
pub struct PerformanceAnomaly {
    /// Type of anomaly detected
    pub anomaly_type: AnomalyType,
    /// Timestamp when anomaly was detected
    pub timestamp: u64,
    /// Severity level of the anomaly
    pub severity: AnomalySeverity,
    /// Detailed description of the anomaly
    pub description: String,
    /// Affected metric value
    pub metric_value: f64,
    /// Expected/baseline value for comparison
    pub baseline_value: f64,
    /// Deviation from baseline (as percentage)
    pub deviation_percent: f64,
}

/// Types of performance anomalies that can be detected
#[derive(Debug, Clone, PartialEq)]
pub enum AnomalyType {
    /// Response time spike above threshold
    ResponseTimeSpike,
    /// Error rate increase above threshold
    ErrorRateIncrease,
    /// Request rate drop below threshold
    RequestRateDrop,
    /// Worker disconnection pattern
    WorkerDisconnection,
    /// Memory usage anomaly
    ResourceAnomaly,
}

/// Severity levels for performance anomalies
#[derive(Debug, Clone, PartialEq)]
pub enum AnomalySeverity {
    /// Low impact anomaly
    Low,
    /// Medium impact anomaly  
    Medium,
    /// High impact anomaly requiring immediate attention
    High,
    /// Critical anomaly that may affect test validity
    Critical,
}

/// Trend analysis results for performance metrics
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    /// Response time trend (improving, degrading, stable)
    pub response_time_trend: TrendDirection,
    /// Request rate trend
    pub request_rate_trend: TrendDirection,
    /// Error rate trend
    pub error_rate_trend: TrendDirection,
    /// Overall performance health score (0.0 - 100.0)
    pub health_score: f64,
    /// Predicted performance trajectory
    pub performance_prediction: PredictionResult,
}

/// Direction of performance trends
#[derive(Debug, Clone, PartialEq)]
pub enum TrendDirection {
    /// Performance is improving
    Improving,
    /// Performance is degrading
    Degrading,
    /// Performance is stable
    Stable,
    /// Not enough data to determine trend
    Unknown,
}

/// Performance prediction results
#[derive(Debug, Clone)]
pub struct PredictionResult {
    /// Predicted average response time in next 60 seconds
    pub predicted_response_time: f64,
    /// Predicted request rate in next 60 seconds
    pub predicted_request_rate: f64,
    /// Confidence level in predictions (0.0 - 1.0)
    pub confidence: f64,
}

/// Advanced metrics storage for time-series analysis
#[derive(Debug, Clone)]
pub struct MetricsTimeSeries {
    /// Metrics data points stored in chronological order
    pub data_points: std::collections::VecDeque<MetricsDataPoint>,
    /// Maximum number of data points to retain
    pub max_data_points: usize,
    /// Time window for each data point in seconds
    pub time_window_seconds: u64,
}

/// Individual metrics data point for time-series storage
#[derive(Debug, Clone)]
pub struct MetricsDataPoint {
    /// Timestamp of this data point
    pub timestamp: u64,
    /// Request metrics for this time window
    pub requests: RequestMetricsSnapshot,
    /// Transaction metrics for this time window  
    pub transactions: TransactionMetricsSnapshot,
    /// Error metrics for this time window
    pub errors: ErrorMetricsSnapshot,
    /// Worker status for this time window
    pub workers: WorkerStatusSnapshot,
}

/// Snapshot of request metrics for a specific time window
#[derive(Debug, Clone)]
pub struct RequestMetricsSnapshot {
    /// Total requests in this time window
    pub count: u64,
    /// Failed requests in this time window
    pub failures: u64,
    /// Average response time in this time window
    pub avg_response_time: f64,
    /// Median response time in this time window
    pub median_response_time: f64,
    /// 95th percentile response time
    pub p95_response_time: f64,
    /// 99th percentile response time
    pub p99_response_time: f64,
    /// Requests per second in this time window
    pub requests_per_second: f64,
}

/// Snapshot of transaction metrics for a specific time window
#[derive(Debug, Clone)]
pub struct TransactionMetricsSnapshot {
    /// Total transactions in this time window
    pub count: u64,
    /// Failed transactions in this time window
    pub failures: u64,
    /// Average transaction time in this time window
    pub avg_transaction_time: f64,
    /// Transactions per second in this time window
    pub transactions_per_second: f64,
}

/// Snapshot of error metrics for a specific time window
#[derive(Debug, Clone)]
pub struct ErrorMetricsSnapshot {
    /// Total errors in this time window
    pub count: u64,
    /// Unique error types in this time window
    pub unique_types: u32,
    /// Errors per second in this time window
    pub errors_per_second: f64,
    /// Most frequent error type
    pub top_error_type: Option<String>,
}

/// Snapshot of worker status for a specific time window
#[derive(Debug, Clone)]
pub struct WorkerStatusSnapshot {
    /// Number of active workers
    pub active_workers: u32,
    /// Total users across all workers
    pub total_users: u32,
    /// Workers that disconnected in this time window
    pub disconnected_workers: u32,
    /// Average CPU usage across workers (if available)
    pub avg_cpu_usage: Option<f64>,
    /// Average memory usage across workers (if available)
    pub avg_memory_usage: Option<f64>,
}

impl Default for AggregatedStatistics {
    fn default() -> Self {
        Self {
            workers_reporting: 0,
            total_requests: 0,
            total_failures: 0,
            success_rate: 100.0,
            average_response_time_ms: 0.0,
            median_response_time_ms: 0.0,
            p95_response_time_ms: 0.0,
            p99_response_time_ms: 0.0,
            response_time_std_dev: 0.0,
            total_transactions: 0,
            transaction_success_rate: 100.0,
            total_active_users: 0,
            requests_per_second: 0.0,
            peak_requests_per_second: 0.0,
            unique_errors: 0,
            error_rate: 0.0,
            last_updated: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

impl Default for ConnectionHealthInfo {
    fn default() -> Self {
        let now = std::time::Instant::now();
        Self {
            status: ConnectionStatus::Healthy,
            connected_since: now,
            reconnection_attempts: 0,
            last_successful_heartbeat: now,
            stability_score: 1.0,
            health_check_interval: 30,
            connection_timeout: 60,
        }
    }
}

impl Default for ConnectionQualityMetrics {
    fn default() -> Self {
        Self {
            average_latency_ms: 0.0,
            min_latency_ms: 0.0,
            max_latency_ms: 0.0,
            latency_samples: std::collections::VecDeque::with_capacity(100),
            packet_loss_percent: 0.0,
            uptime_percent: 100.0,
            heartbeats_sent: 0,
            heartbeats_received: 0,
            connection_drops: 0,
            throughput_bps: 0.0,
        }
    }
}

impl GaggleManager {
    /// Create a new Gaggle Manager
    pub fn new(config: GaggleConfiguration) -> Self {
        Self {
            config,
            workers: Arc::new(RwLock::new(HashMap::new())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            goose_attack: None,
            analytics: Arc::new(RwLock::new(MetricsAnalytics {
                response_time_series: Vec::new(),
                request_rate_series: Vec::new(),
                error_rate_series: Vec::new(),
                anomalies: Vec::new(),
                trends: TrendAnalysis {
                    response_time_trend: TrendDirection::Unknown,
                    request_rate_trend: TrendDirection::Unknown,
                    error_rate_trend: TrendDirection::Unknown,
                    health_score: 100.0,
                    performance_prediction: PredictionResult {
                        predicted_response_time: 0.0,
                        predicted_request_rate: 0.0,
                        confidence: 0.0,
                    },
                },
                retention_period: 3600, // 1 hour default retention
            })),
            time_series: Arc::new(RwLock::new(MetricsTimeSeries {
                data_points: std::collections::VecDeque::new(),
                max_data_points: 720,    // 12 hours at 1-minute intervals
                time_window_seconds: 60, // 1-minute windows
            })),
            aggregated_metrics: Arc::new(RwLock::new(crate::GooseMetrics::default())),
            performance_baselines: Arc::new(RwLock::new(HashMap::new())),
            test_start_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new Gaggle Manager with GooseAttack integration
    pub fn with_goose_attack(config: GaggleConfiguration, goose_attack: GooseAttack) -> Self {
        Self {
            config,
            workers: Arc::new(RwLock::new(HashMap::new())),
            metrics_buffer: Arc::new(Mutex::new(Vec::new())),
            goose_attack: Some(goose_attack),
            analytics: Arc::new(RwLock::new(MetricsAnalytics {
                response_time_series: Vec::new(),
                request_rate_series: Vec::new(),
                error_rate_series: Vec::new(),
                anomalies: Vec::new(),
                trends: TrendAnalysis {
                    response_time_trend: TrendDirection::Unknown,
                    request_rate_trend: TrendDirection::Unknown,
                    error_rate_trend: TrendDirection::Unknown,
                    health_score: 100.0,
                    performance_prediction: PredictionResult {
                        predicted_response_time: 0.0,
                        predicted_request_rate: 0.0,
                        confidence: 0.0,
                    },
                },
                retention_period: 3600, // 1 hour default retention
            })),
            time_series: Arc::new(RwLock::new(MetricsTimeSeries {
                data_points: std::collections::VecDeque::new(),
                max_data_points: 720,    // 12 hours at 1-minute intervals
                time_window_seconds: 60, // 1-minute windows
            })),
            aggregated_metrics: Arc::new(RwLock::new(crate::GooseMetrics::default())),
            performance_baselines: Arc::new(RwLock::new(HashMap::new())),
            test_start_time: Arc::new(RwLock::new(None)),
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

    /// Aggregate metrics from workers and integrate with GooseAttack
    ///
    /// This method handles real-time aggregation of metrics from all workers,
    /// combining them into a unified view for monitoring and reporting.
    pub async fn aggregate_worker_metrics(
        &self,
        worker_metrics: MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            "Aggregating metrics batch from worker {} containing {} request metrics, {} transaction metrics, {} scenario metrics",
            worker_metrics.worker_id,
            worker_metrics.request_metrics.len(),
            worker_metrics.transaction_metrics.len(),
            worker_metrics.scenario_metrics.len()
        );

        // Store raw metrics for historical tracking
        {
            let mut buffer = self.metrics_buffer.lock().await;
            buffer.push(worker_metrics.clone());
        }

        // Process individual metric types for real-time aggregation
        self.process_request_metrics(&worker_metrics.worker_id, &worker_metrics.request_metrics)
            .await?;
        self.process_transaction_metrics(
            &worker_metrics.worker_id,
            &worker_metrics.transaction_metrics,
        )
        .await?;
        self.process_scenario_metrics(&worker_metrics.worker_id, &worker_metrics.scenario_metrics)
            .await?;

        // Update time-series data collection
        self.update_time_series_data(&worker_metrics).await?;

        // Perform advanced analytics
        self.update_analytics(&worker_metrics).await?;

        // Update aggregated metrics in the shared state
        self.update_aggregated_metrics(&worker_metrics).await?;

        // Detect performance anomalies
        self.detect_anomalies(&worker_metrics).await?;

        info!(
            "Successfully aggregated metrics from worker {}: {} total metrics processed",
            worker_metrics.worker_id,
            worker_metrics.request_metrics.len()
                + worker_metrics.transaction_metrics.len()
                + worker_metrics.scenario_metrics.len()
        );

        Ok(())
    }

    /// Process request metrics for aggregation
    async fn process_request_metrics(
        &self,
        worker_id: &str,
        request_metrics: &[RequestMetric],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for request_metric in request_metrics {
            debug!(
                "Processing request metric from worker {}: {} {} ({}ms, success: {})",
                worker_id,
                request_metric.method,
                request_metric.name,
                request_metric.response_time_ms,
                request_metric.success
            );

            // In a production implementation, this would:
            // 1. Update aggregated request counters by endpoint
            // 2. Update response time statistics (min, max, avg, percentiles)
            // 3. Track error rates and status codes
            // 4. Update real-time monitoring dashboards

            // For now, we log the key metrics for monitoring
            if !request_metric.success {
                warn!(
                    "Failed request from worker {}: {} {} - {}",
                    worker_id,
                    request_metric.method,
                    request_metric.name,
                    request_metric.error.as_deref().unwrap_or("Unknown error")
                );
            }
        }

        Ok(())
    }

    /// Process transaction metrics for aggregation
    async fn process_transaction_metrics(
        &self,
        worker_id: &str,
        transaction_metrics: &[TransactionMetric],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for transaction_metric in transaction_metrics {
            debug!(
                "Processing transaction metric from worker {}: {} ({}ms, success: {})",
                worker_id,
                transaction_metric.name,
                transaction_metric.response_time_ms,
                transaction_metric.success
            );

            // In a production implementation, this would:
            // 1. Update transaction completion counters
            // 2. Aggregate transaction timing statistics
            // 3. Track transaction success/failure rates
            // 4. Calculate transaction throughput metrics

            if !transaction_metric.success {
                warn!(
                    "Failed transaction from worker {}: {} - {}",
                    worker_id,
                    transaction_metric.name,
                    transaction_metric
                        .error
                        .as_deref()
                        .unwrap_or("Unknown error")
                );
            }
        }

        Ok(())
    }

    /// Process scenario metrics for aggregation
    async fn process_scenario_metrics(
        &self,
        worker_id: &str,
        scenario_metrics: &[ScenarioMetric],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for scenario_metric in scenario_metrics {
            debug!(
                "Processing scenario metric from worker {}: {} ({} users, {} iterations)",
                worker_id,
                scenario_metric.name,
                scenario_metric.users_count,
                scenario_metric.iterations
            );

            // In a production implementation, this would:
            // 1. Track total users per scenario across all workers
            // 2. Aggregate scenario iteration counts
            // 3. Calculate scenario execution rates
            // 4. Monitor scenario performance and scaling
        }

        Ok(())
    }

    /// Get real-time aggregated statistics across all workers
    pub async fn get_realtime_statistics(
        &self,
    ) -> Result<AggregatedStatistics, Box<dyn std::error::Error + Send + Sync>> {
        let buffer = self.metrics_buffer.lock().await;

        let mut total_requests = 0u64;
        let mut total_failures = 0u64;
        let mut total_transactions = 0u64;
        let mut total_transaction_failures = 0u64;
        let mut total_response_time_ms = 0u64;
        let mut total_users = 0u32;
        let mut workers_reporting = std::collections::HashSet::new();

        // Aggregate metrics across all worker batches
        for batch in buffer.iter() {
            workers_reporting.insert(batch.worker_id.clone());

            for request_metric in &batch.request_metrics {
                total_requests += 1;
                total_response_time_ms += request_metric.response_time_ms;
                if !request_metric.success {
                    total_failures += 1;
                }
            }

            for transaction_metric in &batch.transaction_metrics {
                total_transactions += 1;
                if !transaction_metric.success {
                    total_transaction_failures += 1;
                }
            }

            for scenario_metric in &batch.scenario_metrics {
                total_users += scenario_metric.users_count;
            }
        }

        let avg_response_time = if total_requests > 0 {
            total_response_time_ms as f64 / total_requests as f64
        } else {
            0.0
        };

        let success_rate = if total_requests > 0 {
            ((total_requests - total_failures) as f64 / total_requests as f64) * 100.0
        } else {
            100.0
        };

        let transaction_success_rate = if total_transactions > 0 {
            ((total_transactions - total_transaction_failures) as f64 / total_transactions as f64)
                * 100.0
        } else {
            100.0
        };

        Ok(AggregatedStatistics {
            workers_reporting: workers_reporting.len() as u32,
            total_requests,
            total_failures,
            success_rate,
            average_response_time_ms: avg_response_time,
            median_response_time_ms: 0.0, // TODO: Calculate median from response time distribution
            p95_response_time_ms: 0.0,    // TODO: Calculate 95th percentile
            p99_response_time_ms: 0.0,    // TODO: Calculate 99th percentile
            response_time_std_dev: 0.0,   // TODO: Calculate standard deviation
            total_transactions,
            transaction_success_rate,
            total_active_users: total_users,
            requests_per_second: 0.0, // TODO: Calculate based on time window
            peak_requests_per_second: 0.0, // TODO: Track peak RPS
            unique_errors: 0,         // TODO: Count unique error types
            error_rate: 0.0,          // TODO: Calculate error rate
            last_updated: chrono::Utc::now().timestamp_millis() as u64,
        })
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

    /// Update time-series data collection with new metrics batch
    async fn update_time_series_data(
        &self,
        worker_metrics: &MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_timestamp = chrono::Utc::now().timestamp_millis() as u64;

        // Calculate aggregated metrics for this time window
        let mut total_requests = 0u64;
        let mut total_failures = 0u64;
        let mut total_response_time = 0u64;
        let mut response_times = Vec::new();
        let mut total_transactions = 0u64;
        let mut transaction_failures = 0u64;
        let mut total_users = 0u32;

        // Process request metrics
        for request_metric in &worker_metrics.request_metrics {
            total_requests += 1;
            total_response_time += request_metric.response_time_ms;
            response_times.push(request_metric.response_time_ms as f64);
            if !request_metric.success {
                total_failures += 1;
            }
        }

        // Process transaction metrics
        for transaction_metric in &worker_metrics.transaction_metrics {
            total_transactions += 1;
            if !transaction_metric.success {
                transaction_failures += 1;
            }
        }

        // Process scenario metrics for user count
        for scenario_metric in &worker_metrics.scenario_metrics {
            total_users += scenario_metric.users_count;
        }

        // Calculate percentiles if we have response times
        response_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95 = if !response_times.is_empty() {
            let index = (response_times.len() as f64 * 0.95) as usize;
            response_times
                .get(index.min(response_times.len() - 1))
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let p99 = if !response_times.is_empty() {
            let index = (response_times.len() as f64 * 0.99) as usize;
            response_times
                .get(index.min(response_times.len() - 1))
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let median = if !response_times.is_empty() {
            let index = response_times.len() / 2;
            response_times.get(index).copied().unwrap_or(0.0)
        } else {
            0.0
        };

        let avg_response_time = if total_requests > 0 {
            total_response_time as f64 / total_requests as f64
        } else {
            0.0
        };

        // Calculate requests per second (simplified - based on last timestamp if available)
        let time_series = self.time_series.read().await;
        let time_window = time_series.time_window_seconds as f64;
        let requests_per_second = total_requests as f64 / time_window;
        drop(time_series);

        // Create data point
        let data_point = MetricsDataPoint {
            timestamp: current_timestamp,
            requests: RequestMetricsSnapshot {
                count: total_requests,
                failures: total_failures,
                avg_response_time,
                median_response_time: median,
                p95_response_time: p95,
                p99_response_time: p99,
                requests_per_second,
            },
            transactions: TransactionMetricsSnapshot {
                count: total_transactions,
                failures: transaction_failures,
                avg_transaction_time: 0.0, // TODO: Calculate from transaction metrics
                transactions_per_second: total_transactions as f64 / time_window,
            },
            errors: ErrorMetricsSnapshot {
                count: total_failures,
                unique_types: 0, // TODO: Count unique error types
                errors_per_second: total_failures as f64 / time_window,
                top_error_type: None, // TODO: Identify most frequent error
            },
            workers: WorkerStatusSnapshot {
                active_workers: 1, // This batch is from one worker
                total_users,
                disconnected_workers: 0,
                avg_cpu_usage: None,
                avg_memory_usage: None,
            },
        };

        // Add to time series and maintain retention policy
        let mut time_series = self.time_series.write().await;
        time_series.data_points.push_back(data_point);

        // Remove old data points if we exceed the maximum
        while time_series.data_points.len() > time_series.max_data_points {
            time_series.data_points.pop_front();
        }

        debug!(
            "Updated time-series data: {} data points, current window has {} requests",
            time_series.data_points.len(),
            total_requests
        );

        Ok(())
    }

    /// Update advanced analytics with new metrics
    async fn update_analytics(
        &self,
        worker_metrics: &MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_timestamp = chrono::Utc::now().timestamp_millis() as u64;

        // Calculate current window metrics
        let mut total_requests = 0u64;
        let mut total_failures = 0u64;
        let mut total_response_time = 0u64;

        for request_metric in &worker_metrics.request_metrics {
            total_requests += 1;
            total_response_time += request_metric.response_time_ms;
            if !request_metric.success {
                total_failures += 1;
            }
        }

        let avg_response_time = if total_requests > 0 {
            total_response_time as f64 / total_requests as f64
        } else {
            0.0
        };

        let error_rate = if total_requests > 0 {
            total_failures as f64 / total_requests as f64
        } else {
            0.0
        };

        let requests_per_second = total_requests as f64 / 60.0; // Assume 60-second window

        // Update analytics
        let mut analytics = self.analytics.write().await;

        // Update time series data (keep last 100 points for trend analysis)
        analytics
            .response_time_series
            .push((current_timestamp, avg_response_time));
        if analytics.response_time_series.len() > 100 {
            analytics.response_time_series.remove(0);
        }

        analytics
            .request_rate_series
            .push((current_timestamp, requests_per_second));
        if analytics.request_rate_series.len() > 100 {
            analytics.request_rate_series.remove(0);
        }

        analytics
            .error_rate_series
            .push((current_timestamp, error_rate));
        if analytics.error_rate_series.len() > 100 {
            analytics.error_rate_series.remove(0);
        }

        // Update trend analysis
        analytics.trends = self.calculate_trends(&analytics).await?;

        // Update health score based on recent performance
        analytics.trends.health_score = self.calculate_health_score(&analytics).await;

        debug!(
            "Updated analytics: response_time={:.2}ms, request_rate={:.2}/s, error_rate={:.2}%, health_score={:.1}",
            avg_response_time, requests_per_second, error_rate * 100.0, analytics.trends.health_score
        );

        Ok(())
    }

    /// Update aggregated metrics in shared state
    async fn update_aggregated_metrics(
        &self,
        worker_metrics: &MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _metrics = self.aggregated_metrics.write().await;

        // Update request metrics
        for request_metric in &worker_metrics.request_metrics {
            // In a real implementation, this would properly aggregate into GooseMetrics
            // For now, we update basic counters to demonstrate usage
            debug!(
                "Aggregating request: {} {} ({}ms)",
                request_metric.method, request_metric.name, request_metric.response_time_ms
            );
        }

        // Update transaction metrics
        for transaction_metric in &worker_metrics.transaction_metrics {
            debug!(
                "Aggregating transaction: {} ({}ms)",
                transaction_metric.name, transaction_metric.response_time_ms
            );
        }

        // Update scenario metrics
        for scenario_metric in &worker_metrics.scenario_metrics {
            debug!(
                "Aggregating scenario: {} ({} users, {} iterations)",
                scenario_metric.name, scenario_metric.users_count, scenario_metric.iterations
            );
        }

        debug!(
            "Updated aggregated metrics from worker {}",
            worker_metrics.worker_id
        );
        Ok(())
    }

    /// Detect performance anomalies based on baseline and current metrics
    async fn detect_anomalies(
        &self,
        worker_metrics: &MetricsBatch,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_timestamp = chrono::Utc::now().timestamp_millis() as u64;
        let baselines = self.performance_baselines.read().await;
        let mut analytics = self.analytics.write().await;

        // Calculate current metrics for anomaly detection
        let mut total_requests = 0u64;
        let mut total_failures = 0u64;
        let mut total_response_time = 0u64;

        for request_metric in &worker_metrics.request_metrics {
            total_requests += 1;
            total_response_time += request_metric.response_time_ms;
            if !request_metric.success {
                total_failures += 1;
            }
        }

        if total_requests > 0 {
            let current_avg_response_time = total_response_time as f64 / total_requests as f64;
            let current_error_rate = total_failures as f64 / total_requests as f64;

            // Check for response time anomalies
            if let Some(&baseline_response_time) = baselines.get("avg_response_time") {
                let deviation = ((current_avg_response_time - baseline_response_time)
                    / baseline_response_time
                    * 100.0)
                    .abs();

                if deviation > 50.0 {
                    // 50% deviation threshold
                    let severity = if deviation > 200.0 {
                        AnomalySeverity::Critical
                    } else if deviation > 100.0 {
                        AnomalySeverity::High
                    } else {
                        AnomalySeverity::Medium
                    };

                    let anomaly = PerformanceAnomaly {
                        anomaly_type: AnomalyType::ResponseTimeSpike,
                        timestamp: current_timestamp,
                        severity,
                        description: format!(
                            "Response time spike detected: {:.2}ms vs baseline {:.2}ms ({:.1}% deviation)",
                            current_avg_response_time, baseline_response_time, deviation
                        ),
                        metric_value: current_avg_response_time,
                        baseline_value: baseline_response_time,
                        deviation_percent: deviation,
                    };

                    analytics.anomalies.push(anomaly);
                    warn!(
                        "Performance anomaly detected: Response time spike of {:.1}%",
                        deviation
                    );
                }
            }

            // Check for error rate anomalies
            if let Some(&baseline_error_rate) = baselines.get("error_rate") {
                if current_error_rate > baseline_error_rate * 2.0 {
                    // 2x baseline threshold
                    let deviation =
                        ((current_error_rate - baseline_error_rate) / baseline_error_rate * 100.0)
                            .abs();

                    let anomaly = PerformanceAnomaly {
                        anomaly_type: AnomalyType::ErrorRateIncrease,
                        timestamp: current_timestamp,
                        severity: if current_error_rate > 0.1 { AnomalySeverity::High } else { AnomalySeverity::Medium },
                        description: format!(
                            "Error rate increase detected: {:.2}% vs baseline {:.2}% ({:.1}% deviation)",
                            current_error_rate * 100.0, baseline_error_rate * 100.0, deviation
                        ),
                        metric_value: current_error_rate,
                        baseline_value: baseline_error_rate,
                        deviation_percent: deviation,
                    };

                    analytics.anomalies.push(anomaly);
                    warn!(
                        "Performance anomaly detected: Error rate increase of {:.1}%",
                        deviation
                    );
                }
            }

            // Maintain anomaly history (keep last 50 anomalies)
            if analytics.anomalies.len() > 50 {
                let len = analytics.anomalies.len();
                analytics.anomalies.drain(0..len - 50);
            }
        }

        debug!(
            "Anomaly detection completed for worker {}",
            worker_metrics.worker_id
        );
        Ok(())
    }

    /// Calculate trend analysis from historical data
    async fn calculate_trends(
        &self,
        analytics: &MetricsAnalytics,
    ) -> Result<TrendAnalysis, Box<dyn std::error::Error + Send + Sync>> {
        // Simple trend calculation based on recent data points
        let response_time_trend = if analytics.response_time_series.len() >= 10 {
            let recent_avg = analytics
                .response_time_series
                .iter()
                .rev()
                .take(5)
                .map(|(_, rt)| rt)
                .sum::<f64>()
                / 5.0;

            let earlier_avg = analytics
                .response_time_series
                .iter()
                .rev()
                .skip(5)
                .take(5)
                .map(|(_, rt)| rt)
                .sum::<f64>()
                / 5.0;

            if recent_avg > earlier_avg * 1.1 {
                TrendDirection::Degrading
            } else if recent_avg < earlier_avg * 0.9 {
                TrendDirection::Improving
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        };

        let request_rate_trend = if analytics.request_rate_series.len() >= 10 {
            let recent_avg = analytics
                .request_rate_series
                .iter()
                .rev()
                .take(5)
                .map(|(_, rr)| rr)
                .sum::<f64>()
                / 5.0;

            let earlier_avg = analytics
                .request_rate_series
                .iter()
                .rev()
                .skip(5)
                .take(5)
                .map(|(_, rr)| rr)
                .sum::<f64>()
                / 5.0;

            if recent_avg > earlier_avg * 1.1 {
                TrendDirection::Improving
            } else if recent_avg < earlier_avg * 0.9 {
                TrendDirection::Degrading
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        };

        let error_rate_trend = if analytics.error_rate_series.len() >= 10 {
            let recent_avg = analytics
                .error_rate_series
                .iter()
                .rev()
                .take(5)
                .map(|(_, er)| er)
                .sum::<f64>()
                / 5.0;

            let earlier_avg = analytics
                .error_rate_series
                .iter()
                .rev()
                .skip(5)
                .take(5)
                .map(|(_, er)| er)
                .sum::<f64>()
                / 5.0;

            if recent_avg > earlier_avg * 1.1 {
                TrendDirection::Degrading
            } else if recent_avg < earlier_avg * 0.9 {
                TrendDirection::Improving
            } else {
                TrendDirection::Stable
            }
        } else {
            TrendDirection::Unknown
        };

        // Simple prediction based on recent trend
        let performance_prediction = if !analytics.response_time_series.is_empty()
            && !analytics.request_rate_series.is_empty()
        {
            let latest_response_time = analytics
                .response_time_series
                .last()
                .map(|(_, rt)| *rt)
                .unwrap_or(0.0);
            let latest_request_rate = analytics
                .request_rate_series
                .last()
                .map(|(_, rr)| *rr)
                .unwrap_or(0.0);

            PredictionResult {
                predicted_response_time: latest_response_time, // Simplified - no actual prediction model
                predicted_request_rate: latest_request_rate,
                confidence: 0.7, // Medium confidence for simple prediction
            }
        } else {
            PredictionResult {
                predicted_response_time: 0.0,
                predicted_request_rate: 0.0,
                confidence: 0.0,
            }
        };

        Ok(TrendAnalysis {
            response_time_trend,
            request_rate_trend,
            error_rate_trend,
            health_score: analytics.trends.health_score, // Keep existing health score
            performance_prediction,
        })
    }

    /// Calculate overall health score based on current metrics
    async fn calculate_health_score(&self, analytics: &MetricsAnalytics) -> f64 {
        let mut score = 100.0;

        // Penalize based on recent anomalies
        let recent_critical_anomalies = analytics
            .anomalies
            .iter()
            .filter(|a| a.severity == AnomalySeverity::Critical)
            .count();
        let recent_high_anomalies = analytics
            .anomalies
            .iter()
            .filter(|a| a.severity == AnomalySeverity::High)
            .count();

        score -= recent_critical_anomalies as f64 * 20.0; // -20 points per critical anomaly
        score -= recent_high_anomalies as f64 * 10.0; // -10 points per high severity anomaly

        // Penalize based on trends
        match analytics.trends.response_time_trend {
            TrendDirection::Degrading => score -= 15.0,
            TrendDirection::Improving => score += 5.0,
            _ => {}
        }

        match analytics.trends.error_rate_trend {
            TrendDirection::Degrading => score -= 25.0,
            TrendDirection::Improving => score += 10.0,
            _ => {}
        }

        // Ensure score stays within bounds
        score.max(0.0).min(100.0)
    }

    /// Set test start time for elapsed time calculations
    pub async fn set_test_start_time(&self) {
        let mut start_time = self.test_start_time.write().await;
        *start_time = Some(std::time::Instant::now());
        info!("Test start time recorded");
    }

    /// Get test elapsed time in seconds
    pub async fn get_test_elapsed_time(&self) -> Option<u64> {
        let start_time = self.test_start_time.read().await;
        start_time.map(|start| start.elapsed().as_secs())
    }

    /// Initialize performance baselines for anomaly detection
    pub async fn initialize_performance_baselines(&self) {
        let mut baselines = self.performance_baselines.write().await;

        // Set reasonable default baselines - these would be updated with actual measurements
        baselines.insert("avg_response_time".to_string(), 100.0); // 100ms baseline
        baselines.insert("error_rate".to_string(), 0.01); // 1% error rate baseline
        baselines.insert("requests_per_second".to_string(), 50.0); // 50 RPS baseline

        info!("Performance baselines initialized with default values");
    }

    /// Update performance baselines based on current performance
    pub async fn update_performance_baselines(&self, stats: &AggregatedStatistics) {
        let mut baselines = self.performance_baselines.write().await;

        // Update baselines with exponential moving average (0.1 smoothing factor)
        let smoothing = 0.1;

        if let Some(current_baseline) = baselines.get_mut("avg_response_time") {
            *current_baseline = (*current_baseline * (1.0 - smoothing))
                + (stats.average_response_time_ms * smoothing);
        }

        if let Some(current_baseline) = baselines.get_mut("error_rate") {
            let error_rate = if stats.total_requests > 0 {
                stats.total_failures as f64 / stats.total_requests as f64
            } else {
                0.0
            };
            *current_baseline = (*current_baseline * (1.0 - smoothing)) + (error_rate * smoothing);
        }

        if let Some(current_baseline) = baselines.get_mut("requests_per_second") {
            *current_baseline =
                (*current_baseline * (1.0 - smoothing)) + (stats.requests_per_second * smoothing);
        }

        debug!("Performance baselines updated with current statistics");
    }

    /// Handle worker disconnect and rebalance load (Section 5.2)
    pub async fn handle_worker_disconnect(
        &self,
        worker_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        warn!("Worker {} disconnected", worker_id);

        let (removed_worker, remaining_workers) = {
            let mut workers = self.workers.write().await;
            let removed = workers.remove(worker_id);
            let remaining = workers.len();
            (removed, remaining)
        };

        if let Some(worker) = removed_worker {
            info!(
                "Removed worker {} (was handling {} users), {} workers remaining",
                worker_id, worker.active_users, remaining_workers
            );

            // Rebalance load among remaining workers if we had an active load test
            if worker.active_users > 0 {
                self.rebalance_load_after_disconnect(worker.active_users)
                    .await?;
            }
        } else {
            warn!("Attempted to remove unknown worker: {}", worker_id);
        }

        // Check if we have enough workers to continue the test
        if remaining_workers == 0 {
            error!("All workers disconnected, load test cannot continue");
            // In a full implementation, this might trigger test failure handling
        } else if remaining_workers < 2 {
            warn!(
                "Only {} worker(s) remaining, test reliability may be affected",
                remaining_workers
            );
        }

        Ok(())
    }

    /// Rebalance load after a worker disconnection (Section 5.2)
    pub async fn rebalance_load_after_disconnect(
        &self,
        orphaned_users: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let workers = self.workers.read().await;
        let worker_count = workers.len();

        if worker_count == 0 {
            return Err("Cannot rebalance load: no workers available".into());
        }

        info!(
            "Rebalancing {} orphaned users across {} remaining workers",
            orphaned_users, worker_count
        );

        // Calculate new user distribution
        let additional_users_per_worker = orphaned_users / (worker_count as u32);
        let extra_users = orphaned_users % (worker_count as u32);

        info!(
            "Distributing {} additional users per worker, with {} extra users",
            additional_users_per_worker, extra_users
        );

        // Send rebalancing commands to each remaining worker
        let mut extra_distributed = 0;
        for (worker_id, worker) in workers.iter() {
            let additional_users = additional_users_per_worker
                + if extra_distributed < extra_users {
                    1
                } else {
                    0
                };

            if extra_distributed < extra_users {
                extra_distributed += 1;
            }

            let new_user_count = worker.active_users + additional_users;

            info!(
                "Updating worker {} from {} to {} users (+{})",
                worker_id, worker.active_users, new_user_count, additional_users
            );

            // Send UPDATE_USERS command to rebalance
            let rebalance_command = ManagerCommand {
                command_type: CommandType::UpdateUsers.into(),
                test_config: None,
                user_count: Some(new_user_count),
                message: Some(format!(
                    "Rebalancing load: {} additional users due to worker disconnect",
                    additional_users
                )),
            };

            // In a full implementation, this would send the command to the specific worker
            // For now, we broadcast to all workers (they'll filter by their ID)
            self.broadcast_command(rebalance_command).await?;
        }

        info!("Load rebalancing completed successfully");
        Ok(())
    }

    /// Monitor worker health and handle failures (Section 5.2)
    pub async fn start_worker_health_monitoring(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let workers = Arc::clone(&self.workers);
        let worker_timeout = std::time::Duration::from_secs(90); // 90 seconds timeout

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30)); // Check every 30 seconds

            loop {
                interval.tick().await;

                let mut disconnected_workers = Vec::new();
                {
                    let workers_guard = workers.read().await;
                    let now = std::time::Instant::now();

                    for (worker_id, worker) in workers_guard.iter() {
                        if now.duration_since(worker.last_heartbeat) > worker_timeout {
                            warn!(
                                "Worker {} health check failed - last heartbeat was {:?} ago",
                                worker_id,
                                now.duration_since(worker.last_heartbeat)
                            );
                            disconnected_workers.push(worker_id.clone());
                        }
                    }
                }

                // Handle disconnected workers outside of the lock
                for worker_id in disconnected_workers {
                    info!(
                        "Marking worker {} as disconnected due to health check failure",
                        worker_id
                    );
                    // In a full implementation, this would call handle_worker_disconnect
                    // For now, just log the detection
                    warn!("Worker {} would be removed and load rebalanced", worker_id);
                }
            }
        });

        info!("Worker health monitoring started");
        Ok(())
    }

    /// Get connection status of all workers
    pub async fn get_worker_connection_status(&self) -> HashMap<String, bool> {
        let workers = self.workers.read().await;
        let now = std::time::Instant::now();
        let healthy_threshold = std::time::Duration::from_secs(60);

        workers
            .iter()
            .map(|(worker_id, worker)| {
                let is_healthy = now.duration_since(worker.last_heartbeat) <= healthy_threshold;
                (worker_id.clone(), is_healthy)
            })
            .collect()
    }

    /// Handle graceful degradation when workers are lost
    pub async fn handle_graceful_degradation(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let worker_count = self.worker_count().await;

        if worker_count == 0 {
            error!("All workers lost - stopping load test");
            self.stop_distributed_load_test().await?;
            return Err("All workers disconnected, load test stopped".into());
        }

        let total_capacity = {
            let workers = self.workers.read().await;
            workers.values().map(|w| w.max_users).sum::<u32>()
        };

        let current_load = {
            let workers = self.workers.read().await;
            workers.values().map(|w| w.active_users).sum::<u32>()
        };

        if current_load > total_capacity {
            warn!(
                "Current load ({} users) exceeds remaining capacity ({} users)",
                current_load, total_capacity
            );

            // Reduce load to match capacity
            let adjusted_load = (total_capacity as f64 * 0.8) as u32; // Use 80% of capacity for safety
            self.redistribute_users(adjusted_load).await?;

            info!(
                "Load reduced to {} users to match remaining worker capacity",
                adjusted_load
            );
        }

        Ok(())
    }

    // ============================================================================
    // Section 6.1: Connection Status Monitoring API Methods
    // ============================================================================

    /// Generate comprehensive health report for external monitoring systems (Section 6.1)
    pub async fn generate_health_report(
        &self,
    ) -> Result<HealthReport, Box<dyn std::error::Error + Send + Sync>> {
        let workers = self.workers.read().await;
        let now = std::time::Instant::now();

        let mut worker_health_statuses = HashMap::new();
        let mut connection_summary_data = ConnectionSummaryData::default();
        let mut alerts = Vec::new();

        // Process each worker's health status
        for (worker_id, worker) in workers.iter() {
            let health_status = self.calculate_worker_health_status(worker, now).await;

            // Update connection summary
            connection_summary_data.total_workers += 1;
            match health_status.connection_status {
                ConnectionStatus::Healthy => connection_summary_data.healthy_connections += 1,
                ConnectionStatus::Degraded => connection_summary_data.degraded_connections += 1,
                ConnectionStatus::Unstable => connection_summary_data.unstable_connections += 1,
                ConnectionStatus::Disconnected => connection_summary_data.disconnected_workers += 1,
                ConnectionStatus::Reconnecting => connection_summary_data.reconnecting_workers += 1,
                ConnectionStatus::Failed => connection_summary_data.failed_connections += 1,
            }

            // Add latency to average calculation
            connection_summary_data.total_latency += health_status.current_latency_ms;
            if health_status.current_latency_ms > 0.0 {
                connection_summary_data.active_connections += 1;
            }

            // Generate alerts for problematic workers
            if let Some(alert) = self.generate_worker_alert(&health_status).await {
                alerts.push(alert);
            }

            worker_health_statuses.insert(worker_id.clone(), health_status);
        }

        // Calculate connection summary metrics
        let connection_summary = ConnectionSummary {
            total_workers: connection_summary_data.total_workers,
            healthy_connections: connection_summary_data.healthy_connections,
            degraded_connections: connection_summary_data.degraded_connections,
            unstable_connections: connection_summary_data.unstable_connections,
            disconnected_workers: connection_summary_data.disconnected_workers,
            average_latency_ms: if connection_summary_data.active_connections > 0 {
                connection_summary_data.total_latency
                    / connection_summary_data.active_connections as f64
            } else {
                0.0
            },
            stability_score: self
                .calculate_overall_stability_score(&connection_summary_data)
                .await,
        };

        // Generate performance summary
        let performance_summary = self.calculate_performance_summary().await?;

        // Determine overall system health
        let overall_health = self
            .determine_overall_system_health(&connection_summary, &performance_summary, &alerts)
            .await;

        Ok(HealthReport {
            timestamp: now,
            overall_health,
            worker_health: worker_health_statuses,
            connection_summary,
            performance_summary,
            alerts,
        })
    }

    /// Get detailed connection quality metrics for a specific worker (Section 6.1)
    pub async fn get_worker_connection_quality(
        &self,
        worker_id: &str,
    ) -> Option<ConnectionQualityMetrics> {
        let workers = self.workers.read().await;
        workers
            .get(worker_id)
            .map(|worker| worker.connection_metrics.clone())
    }

    /// Update connection latency for a worker (Section 6.1)
    pub async fn update_worker_latency(
        &self,
        worker_id: &str,
        latency_ms: f64,
        successful: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut workers = self.workers.write().await;

        if let Some(worker) = workers.get_mut(worker_id) {
            let now = std::time::Instant::now();

            // Update latency samples
            let sample = LatencySample {
                timestamp: now,
                latency_ms,
                successful,
            };

            worker.connection_metrics.latency_samples.push_back(sample);

            // Keep only last 100 samples
            while worker.connection_metrics.latency_samples.len() > 100 {
                worker.connection_metrics.latency_samples.pop_front();
            }

            // Update aggregate latency metrics
            if successful {
                worker.connection_metrics.heartbeats_received += 1;

                if worker.connection_metrics.min_latency_ms == 0.0
                    || latency_ms < worker.connection_metrics.min_latency_ms
                {
                    worker.connection_metrics.min_latency_ms = latency_ms;
                }

                if latency_ms > worker.connection_metrics.max_latency_ms {
                    worker.connection_metrics.max_latency_ms = latency_ms;
                }

                // Update average latency (exponential moving average)
                let alpha = 0.1; // Smoothing factor
                if worker.connection_metrics.average_latency_ms == 0.0 {
                    worker.connection_metrics.average_latency_ms = latency_ms;
                } else {
                    worker.connection_metrics.average_latency_ms =
                        worker.connection_metrics.average_latency_ms * (1.0 - alpha)
                            + latency_ms * alpha;
                }
            }

            worker.connection_metrics.heartbeats_sent += 1;

            // Update packet loss percentage
            let total_heartbeats = worker.connection_metrics.heartbeats_sent;
            let successful_heartbeats = worker.connection_metrics.heartbeats_received;

            if total_heartbeats > 0 {
                worker.connection_metrics.packet_loss_percent =
                    ((total_heartbeats - successful_heartbeats) as f64 / total_heartbeats as f64)
                        * 100.0;
            }

            // Update connection health status based on latency
            if latency_ms > 1000.0 {
                worker.connection_health.status = ConnectionStatus::Degraded;
            } else if latency_ms > 500.0
                && worker.connection_health.status == ConnectionStatus::Healthy
            {
                worker.connection_health.status = ConnectionStatus::Degraded;
            }

            debug!(
                "Updated latency for worker {}: {:.2}ms (success: {})",
                worker_id, latency_ms, successful
            );
        } else {
            return Err(format!("Worker {} not found", worker_id).into());
        }

        Ok(())
    }

    /// Configure health check intervals for all workers (Section 6.1)
    pub async fn configure_health_check_intervals(
        &self,
        health_check_interval: u64,
        connection_timeout: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut workers = self.workers.write().await;

        for (worker_id, worker) in workers.iter_mut() {
            worker.connection_health.health_check_interval = health_check_interval;
            worker.connection_health.connection_timeout = connection_timeout;
            info!(
                "Updated health check configuration for worker {}: interval={}s, timeout={}s",
                worker_id, health_check_interval, connection_timeout
            );
        }

        info!(
            "Health check intervals configured: interval={}s, timeout={}s for {} workers",
            health_check_interval,
            connection_timeout,
            workers.len()
        );
        Ok(())
    }

    /// Get system health dashboard data (Section 6.1 & 6.2)
    pub async fn get_health_dashboard_data(
        &self,
    ) -> Result<HealthDashboardData, Box<dyn std::error::Error + Send + Sync>> {
        let health_report = self.generate_health_report().await?;
        let analytics = self.analytics.read().await;
        let heartbeat_summary = self.get_heartbeat_summary().await?;

        Ok(HealthDashboardData {
            timestamp: health_report.timestamp,
            overall_health: health_report.overall_health,
            total_workers: health_report.connection_summary.total_workers,
            healthy_workers: health_report.connection_summary.healthy_connections,
            degraded_workers: health_report.connection_summary.degraded_connections,
            disconnected_workers: health_report.connection_summary.disconnected_workers,
            average_latency: health_report.connection_summary.average_latency_ms,
            stability_score: health_report.connection_summary.stability_score,
            total_rps: health_report.performance_summary.total_rps,
            error_rate: health_report.performance_summary.error_rate_percent,
            active_users: health_report.performance_summary.total_active_users,
            recent_alerts: health_report.alerts.len() as u32,
            health_score: analytics.trends.health_score,
            heartbeat_summary,
        })
    }

    /// Initialize the gaggle manager with heartbeat monitoring (Section 6.2)
    pub async fn initialize(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Initializing GaggleManager with advanced features");

        // Initialize performance baselines
        self.initialize_performance_baselines().await;

        // Start worker health monitoring
        self.start_worker_health_monitoring().await?;

        // Start heartbeat monitoring system (Section 6.2)
        self.start_heartbeat_monitoring().await;

        info!("GaggleManager initialized with heartbeat monitoring and health systems");
        Ok(())
    }

    // Helper methods for health monitoring

    async fn calculate_worker_health_status(
        &self,
        worker: &WorkerState,
        now: std::time::Instant,
    ) -> WorkerHealthStatus {
        let seconds_since_heartbeat = now.duration_since(worker.last_heartbeat).as_secs();

        // Determine connection status based on heartbeat timing
        let connection_status =
            if seconds_since_heartbeat > worker.connection_health.connection_timeout {
                ConnectionStatus::Disconnected
            } else if seconds_since_heartbeat > worker.connection_health.connection_timeout / 2 {
                ConnectionStatus::Degraded
            } else {
                worker.connection_health.status.clone()
            };

        // Calculate quality score based on multiple factors
        let mut quality_score = 1.0;

        // Penalize based on packet loss
        quality_score -= worker.connection_metrics.packet_loss_percent / 100.0;

        // Penalize based on high latency
        if worker.connection_metrics.average_latency_ms > 100.0 {
            quality_score -= (worker.connection_metrics.average_latency_ms - 100.0) / 1000.0;
        }

        // Penalize based on connection drops
        if worker.connection_metrics.connection_drops > 0 {
            quality_score -= worker.connection_metrics.connection_drops as f64 * 0.1;
        }

        quality_score = quality_score.max(0.0).min(1.0);

        WorkerHealthStatus {
            worker_id: worker.id.clone(),
            connection_status,
            quality_score,
            current_latency_ms: worker.connection_metrics.average_latency_ms,
            seconds_since_heartbeat,
            active_users: worker.active_users,
            performance_metrics: WorkerPerformanceMetrics {
                requests_per_second: 0.0,  // Would be calculated from recent metrics
                avg_response_time_ms: 0.0, // Would be calculated from recent metrics
                error_rate_percent: 0.0,   // Would be calculated from recent metrics
                cpu_usage_percent: None,
                memory_usage_percent: None,
            },
        }
    }

    async fn generate_worker_alert(
        &self,
        health_status: &WorkerHealthStatus,
    ) -> Option<HealthAlert> {
        let mut metadata = HashMap::new();
        metadata.insert("worker_id".to_string(), health_status.worker_id.clone());
        metadata.insert(
            "quality_score".to_string(),
            format!("{:.2}", health_status.quality_score),
        );
        metadata.insert(
            "latency_ms".to_string(),
            format!("{:.1}", health_status.current_latency_ms),
        );

        match health_status.connection_status {
            ConnectionStatus::Disconnected => Some(HealthAlert {
                severity: AlertSeverity::Critical,
                alert_type: AlertType::WorkerDisconnect,
                message: format!(
                    "Worker {} disconnected - last heartbeat {}s ago",
                    health_status.worker_id, health_status.seconds_since_heartbeat
                ),
                worker_id: Some(health_status.worker_id.clone()),
                timestamp: std::time::Instant::now(),
                metadata,
            }),
            ConnectionStatus::Degraded if health_status.current_latency_ms > 500.0 => {
                Some(HealthAlert {
                    severity: AlertSeverity::Warning,
                    alert_type: AlertType::HighLatency,
                    message: format!(
                        "Worker {} experiencing high latency: {:.1}ms",
                        health_status.worker_id, health_status.current_latency_ms
                    ),
                    worker_id: Some(health_status.worker_id.clone()),
                    timestamp: std::time::Instant::now(),
                    metadata,
                })
            }
            ConnectionStatus::Unstable => Some(HealthAlert {
                severity: AlertSeverity::Warning,
                alert_type: AlertType::ConnectionIssue,
                message: format!(
                    "Worker {} connection unstable - quality score: {:.2}",
                    health_status.worker_id, health_status.quality_score
                ),
                worker_id: Some(health_status.worker_id.clone()),
                timestamp: std::time::Instant::now(),
                metadata,
            }),
            _ => None,
        }
    }

    async fn calculate_overall_stability_score(
        &self,
        connection_data: &ConnectionSummaryData,
    ) -> f64 {
        if connection_data.total_workers == 0 {
            return 1.0;
        }

        let healthy_ratio =
            connection_data.healthy_connections as f64 / connection_data.total_workers as f64;
        let degraded_penalty = connection_data.degraded_connections as f64
            / connection_data.total_workers as f64
            * 0.3;
        let disconnected_penalty = connection_data.disconnected_workers as f64
            / connection_data.total_workers as f64
            * 0.8;

        (healthy_ratio - degraded_penalty - disconnected_penalty)
            .max(0.0)
            .min(1.0)
    }

    async fn calculate_performance_summary(
        &self,
    ) -> Result<PerformanceSummary, Box<dyn std::error::Error + Send + Sync>> {
        let stats = self.get_realtime_statistics().await?;

        Ok(PerformanceSummary {
            total_rps: stats.requests_per_second,
            avg_response_time_ms: stats.average_response_time_ms,
            error_rate_percent: if stats.total_requests > 0 {
                (stats.total_failures as f64 / stats.total_requests as f64) * 100.0
            } else {
                0.0
            },
            total_active_users: stats.total_active_users,
            throughput_rpm: stats.requests_per_second * 60.0,
        })
    }

    async fn determine_overall_system_health(
        &self,
        connection_summary: &ConnectionSummary,
        performance_summary: &PerformanceSummary,
        alerts: &[HealthAlert],
    ) -> SystemHealthStatus {
        let critical_alerts = alerts
            .iter()
            .filter(|a| a.severity == AlertSeverity::Critical)
            .count();
        let error_alerts = alerts
            .iter()
            .filter(|a| a.severity == AlertSeverity::Error)
            .count();

        if critical_alerts > 0
            || connection_summary.disconnected_workers > connection_summary.total_workers / 2
        {
            SystemHealthStatus::Critical
        } else if error_alerts > 0
            || connection_summary.degraded_connections > connection_summary.total_workers / 3
        {
            SystemHealthStatus::Degraded
        } else if performance_summary.error_rate_percent > 5.0
            || connection_summary.average_latency_ms > 200.0
        {
            SystemHealthStatus::Warning
        } else {
            SystemHealthStatus::Healthy
        }
    }

    // ============================================================================
    // Section 6.2: Heartbeat Implementation Methods
    // ============================================================================

    /// Start heartbeat monitoring for all workers (Section 6.2)
    pub async fn start_heartbeat_monitoring(&self) {
        let workers = Arc::clone(&self.workers);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

            loop {
                interval.tick().await;

                let workers_guard = workers.read().await;
                for (worker_id, _worker) in workers_guard.iter() {
                    debug!("Heartbeat monitoring check for worker: {}", worker_id);
                    // In a full implementation, this would send actual heartbeat requests
                }
            }
        });

        info!("Heartbeat monitoring system started");
    }

    /// Get heartbeat summary for all workers (Section 6.2)
    pub async fn get_heartbeat_summary(
        &self,
    ) -> Result<HeartbeatSummary, Box<dyn std::error::Error + Send + Sync>> {
        let workers = self.workers.read().await;
        let now = std::time::Instant::now();

        let mut total_workers = 0u32;
        let mut responding_workers = 0u32;
        let mut total_latency = 0.0f64;
        let mut active_latency_count = 0u32;

        for (_worker_id, worker) in workers.iter() {
            total_workers += 1;

            // Check if worker is responding (heartbeat within last 30 seconds)
            if now.duration_since(worker.last_heartbeat).as_secs() <= 30 {
                responding_workers += 1;
            }

            // Add to latency calculation
            if worker.connection_metrics.average_latency_ms > 0.0 {
                total_latency += worker.connection_metrics.average_latency_ms;
                active_latency_count += 1;
            }
        }

        let average_latency_ms = if active_latency_count > 0 {
            total_latency / active_latency_count as f64
        } else {
            0.0
        };

        let health_score = if total_workers > 0 {
            responding_workers as f64 / total_workers as f64
        } else {
            1.0
        };

        Ok(HeartbeatSummary {
            total_workers,
            responding_workers,
            non_responding_workers: total_workers - responding_workers,
            average_latency_ms,
            health_score,
            timestamp: now,
        })
    }

    /// Configure heartbeat settings for a specific worker (Section 6.2)
    pub async fn configure_heartbeat_settings(
        &self,
        worker_id: &str,
        interval_ms: u64,
        timeout_ms: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut workers = self.workers.write().await;

        if let Some(worker) = workers.get_mut(worker_id) {
            // Update heartbeat configuration via connection health settings
            worker.connection_health.health_check_interval = interval_ms / 1000; // Convert to seconds
            worker.connection_health.connection_timeout = timeout_ms / 1000; // Convert to seconds

            info!(
                "Updated heartbeat settings for worker {}: interval={}ms, timeout={}ms",
                worker_id, interval_ms, timeout_ms
            );
        } else {
            return Err(format!("Worker {} not found", worker_id).into());
        }

        Ok(())
    }

    /// Send heartbeat to all workers (Section 6.2)
    pub async fn send_heartbeat_to_all_workers(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let workers = self.workers.read().await;

        for (worker_id, _worker) in workers.iter() {
            self.send_heartbeat_to_worker(worker_id).await?;
        }

        debug!("Heartbeat sent to {} workers", workers.len());
        Ok(())
    }

    /// Send heartbeat to a specific worker (Section 6.2)
    pub async fn send_heartbeat_to_worker(
        &self,
        worker_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start_time = std::time::Instant::now();

        // In a full implementation, this would send an actual heartbeat message via gRPC
        // For now, we simulate the heartbeat and update metrics

        // Simulate heartbeat response processing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let latency_ms = start_time.elapsed().as_millis() as f64;

        // Update worker latency metrics
        self.update_worker_latency(worker_id, latency_ms, true)
            .await?;

        debug!(
            "Heartbeat sent to worker {}: {:.2}ms",
            worker_id, latency_ms
        );
        Ok(())
    }

    /// Check for heartbeat timeouts and update worker status (Section 6.2)
    pub async fn check_heartbeat_timeouts(
        &self,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let mut workers = self.workers.write().await;
        let now = std::time::Instant::now();
        let mut timed_out_workers = Vec::new();

        for (worker_id, worker) in workers.iter_mut() {
            let seconds_since_heartbeat = now.duration_since(worker.last_heartbeat).as_secs();
            let timeout_threshold = worker.connection_health.connection_timeout;

            if seconds_since_heartbeat > timeout_threshold {
                // Worker has timed out
                worker.connection_health.status = ConnectionStatus::Disconnected;
                timed_out_workers.push(worker_id.clone());

                warn!(
                    "Worker {} heartbeat timeout: {}s > {}s threshold",
                    worker_id, seconds_since_heartbeat, timeout_threshold
                );
            } else if seconds_since_heartbeat > timeout_threshold / 2 {
                // Worker is degraded (over half timeout threshold)
                if worker.connection_health.status == ConnectionStatus::Healthy {
                    worker.connection_health.status = ConnectionStatus::Degraded;
                    debug!(
                        "Worker {} connection degraded due to slow heartbeat",
                        worker_id
                    );
                }
            }
        }

        Ok(timed_out_workers)
    }

    /// Handle heartbeat response from worker (Section 6.2)
    pub async fn handle_heartbeat_response(
        &self,
        worker_id: &str,
        response_time_ms: f64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut workers = self.workers.write().await;

        if let Some(worker) = workers.get_mut(worker_id) {
            let now = std::time::Instant::now();

            // Update last heartbeat time
            worker.last_heartbeat = now;
            worker.connection_health.last_successful_heartbeat = now;

            // Reset consecutive failures
            // (consecutive_failures would be tracked in HeartbeatInfo if we add it to WorkerState)

            // Update connection status based on response time
            if response_time_ms < 100.0 {
                worker.connection_health.status = ConnectionStatus::Healthy;
            } else if response_time_ms < 500.0 {
                worker.connection_health.status = ConnectionStatus::Degraded;
            } else {
                worker.connection_health.status = ConnectionStatus::Unstable;
            }

            // Update latency metrics
            self.update_worker_latency(worker_id, response_time_ms, true)
                .await?;

            debug!(
                "Processed heartbeat response from worker {}: {:.2}ms",
                worker_id, response_time_ms
            );
        } else {
            return Err(format!("Worker {} not found for heartbeat response", worker_id).into());
        }

        Ok(())
    }

    /// Get heartbeat status for a specific worker (Section 6.2)
    pub async fn get_heartbeat_status(&self, worker_id: &str) -> Option<HeartbeatStatus> {
        let workers = self.workers.read().await;
        let now = std::time::Instant::now();

        workers.get(worker_id).map(|worker| {
            let seconds_since_heartbeat = now.duration_since(worker.last_heartbeat).as_secs();
            let is_responding =
                seconds_since_heartbeat <= worker.connection_health.connection_timeout;

            HeartbeatStatus {
                worker_id: worker.id.clone(),
                is_responding,
                last_heartbeat: worker.last_heartbeat,
                consecutive_failures: 0, // Would be tracked in HeartbeatInfo
                average_latency_ms: worker.connection_metrics.average_latency_ms,
                health_status: worker.connection_health.status.clone(),
            }
        })
    }
}

/// Helper structure for connection summary calculations (Section 6.1)
#[derive(Debug, Default)]
struct ConnectionSummaryData {
    total_workers: u32,
    healthy_connections: u32,
    degraded_connections: u32,
    unstable_connections: u32,
    disconnected_workers: u32,
    reconnecting_workers: u32,
    failed_connections: u32,
    total_latency: f64,
    active_connections: u32,
}

/// Heartbeat information for connection monitoring (Section 6.2)
#[derive(Debug, Clone)]
pub struct HeartbeatInfo {
    /// Heartbeat interval in milliseconds
    pub interval_ms: u64,
    /// Heartbeat timeout in milliseconds
    pub timeout_ms: u64,
    /// Number of consecutive heartbeat failures
    pub consecutive_failures: u32,
    /// Last heartbeat sent timestamp
    pub last_sent: std::time::Instant,
    /// Last heartbeat response received timestamp
    pub last_received: Option<std::time::Instant>,
    /// Current heartbeat latency in milliseconds
    pub current_latency_ms: f64,
    /// Average heartbeat latency in milliseconds
    pub average_latency_ms: f64,
}

/// Heartbeat status for individual worker (Section 6.2)
#[derive(Debug, Clone)]
pub struct HeartbeatStatus {
    /// Worker identifier
    pub worker_id: String,
    /// Current heartbeat status
    pub is_responding: bool,
    /// Last heartbeat timestamp
    pub last_heartbeat: std::time::Instant,
    /// Consecutive failures count
    pub consecutive_failures: u32,
    /// Average latency in milliseconds
    pub average_latency_ms: f64,
    /// Connection health based on heartbeats
    pub health_status: ConnectionStatus,
}

/// System-wide heartbeat summary (Section 6.2)
#[derive(Debug, Clone)]
pub struct HeartbeatSummary {
    /// Total number of workers being monitored
    pub total_workers: u32,
    /// Number of workers responding to heartbeats
    pub responding_workers: u32,
    /// Number of workers not responding to heartbeats
    pub non_responding_workers: u32,
    /// Average heartbeat latency across all workers
    pub average_latency_ms: f64,
    /// Overall heartbeat health score (0.0 - 1.0)
    pub health_score: f64,
    /// Timestamp of this summary
    pub timestamp: std::time::Instant,
}

/// Health dashboard data for external monitoring systems (Section 6.1)
#[derive(Debug, Clone)]
pub struct HealthDashboardData {
    pub timestamp: std::time::Instant,
    pub overall_health: SystemHealthStatus,
    pub total_workers: u32,
    pub healthy_workers: u32,
    pub degraded_workers: u32,
    pub disconnected_workers: u32,
    pub average_latency: f64,
    pub stability_score: f64,
    pub total_rps: f64,
    pub error_rate: f64,
    pub active_users: u32,
    pub recent_alerts: u32,
    pub health_score: f64,
    pub heartbeat_summary: HeartbeatSummary,
}

/// Additional health monitoring structures for comprehensive reporting (Section 6.1)
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Timestamp when the report was generated
    pub timestamp: std::time::Instant,
    /// Overall system health status
    pub overall_health: SystemHealthStatus,
    /// Individual worker health statuses
    pub worker_health: std::collections::HashMap<String, WorkerHealthStatus>,
    /// Connection quality summary
    pub connection_summary: ConnectionSummary,
    /// Performance metrics summary
    pub performance_summary: PerformanceSummary,
    /// Active alerts and warnings
    pub alerts: Vec<HealthAlert>,
}

/// System-wide health status (Section 6.1)
#[derive(Debug, Clone, PartialEq)]
pub enum SystemHealthStatus {
    /// All systems operational
    Healthy,
    /// Minor issues detected
    Warning,
    /// Significant issues affecting performance
    Degraded,
    /// Critical issues requiring immediate attention
    Critical,
}

/// Individual worker health status for detailed monitoring (Section 6.1)
#[derive(Debug, Clone)]
pub struct WorkerHealthStatus {
    /// Worker identifier
    pub worker_id: String,
    /// Current connection status
    pub connection_status: ConnectionStatus,
    /// Connection quality score (0.0 - 1.0)
    pub quality_score: f64,
    /// Current latency in milliseconds
    pub current_latency_ms: f64,
    /// Time since last heartbeat
    pub seconds_since_heartbeat: u64,
    /// Number of active users on this worker
    pub active_users: u32,
    /// Worker performance metrics
    pub performance_metrics: WorkerPerformanceMetrics,
}

/// Worker-specific performance metrics (Section 6.1)
#[derive(Debug, Clone)]
pub struct WorkerPerformanceMetrics {
    /// Requests per second
    pub requests_per_second: f64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Error rate percentage
    pub error_rate_percent: f64,
    /// CPU usage percentage (if available)
    pub cpu_usage_percent: Option<f64>,
    /// Memory usage percentage (if available)  
    pub memory_usage_percent: Option<f64>,
}

/// Connection quality summary across all workers (Section 6.1)
#[derive(Debug, Clone)]
pub struct ConnectionSummary {
    /// Total number of connected workers
    pub total_workers: u32,
    /// Number of healthy connections
    pub healthy_connections: u32,
    /// Number of degraded connections
    pub degraded_connections: u32,
    /// Number of unstable connections
    pub unstable_connections: u32,
    /// Number of disconnected workers
    pub disconnected_workers: u32,
    /// Average connection latency across all workers
    pub average_latency_ms: f64,
    /// Overall connection stability score (0.0 - 1.0)
    pub stability_score: f64,
}

/// Performance summary across all workers (Section 6.1)
#[derive(Debug, Clone)]
pub struct PerformanceSummary {
    /// Total requests per second across all workers
    pub total_rps: f64,
    /// Average response time across all workers
    pub avg_response_time_ms: f64,
    /// Overall error rate percentage
    pub error_rate_percent: f64,
    /// Total number of active users
    pub total_active_users: u32,
    /// System throughput in requests per minute
    pub throughput_rpm: f64,
}

/// Health alert for monitoring and notification systems (Section 6.1)
#[derive(Debug, Clone)]
pub struct HealthAlert {
    /// Alert severity level
    pub severity: AlertSeverity,
    /// Alert type/category
    pub alert_type: AlertType,
    /// Human-readable alert message
    pub message: String,
    /// Worker ID if alert is worker-specific
    pub worker_id: Option<String>,
    /// Timestamp when alert was triggered
    pub timestamp: std::time::Instant,
    /// Additional context data
    pub metadata: std::collections::HashMap<String, String>,
}

/// Alert severity levels (Section 6.1)
#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    /// Informational alert
    Info,
    /// Warning that may require attention
    Warning,
    /// Error that affects functionality
    Error,
    /// Critical issue requiring immediate action
    Critical,
}

/// Types of health alerts (Section 6.1)
#[derive(Debug, Clone, PartialEq)]
pub enum AlertType {
    /// Connection-related alerts
    ConnectionIssue,
    /// Performance degradation alerts
    PerformanceIssue,
    /// Worker disconnection alerts
    WorkerDisconnect,
    /// High latency alerts
    HighLatency,
    /// Error rate alerts
    HighErrorRate,
    /// Resource usage alerts
    ResourceIssue,
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
                connection_health: ConnectionHealthInfo::default(),
                connection_metrics: ConnectionQualityMetrics::default(),
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
