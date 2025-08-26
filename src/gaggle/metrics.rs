//! Gaggle metrics handling and conversion utilities

use super::gaggle_proto::*;
use std::collections::HashMap;

/// Converter for transforming Goose metrics to gaggle protobuf format
pub struct MetricsConverter;

impl MetricsConverter {
    /// Create a metrics batch for testing purposes
    pub fn create_test_batch(worker_id: String) -> MetricsBatch {
        MetricsBatch {
            worker_id,
            request_metrics: vec![],
            transaction_metrics: vec![],
            scenario_metrics: vec![],
            batch_timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    /// Create a sample request metric for testing
    pub fn create_sample_request_metric(name: String, url: String) -> RequestMetric {
        RequestMetric {
            name,
            method: "GET".to_string(),
            url,
            status_code: 200,
            response_time_ms: 100,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            success: true,
            error: None,
        }
    }

    /// Create a sample transaction metric for testing
    pub fn create_sample_transaction_metric(name: String) -> TransactionMetric {
        TransactionMetric {
            name,
            response_time_ms: 150,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            success: true,
            error: None,
        }
    }

    /// Create a sample scenario metric for testing
    pub fn create_sample_scenario_metric(name: String, users_count: u32) -> ScenarioMetric {
        ScenarioMetric {
            name,
            users_count,
            iterations: 1,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

/// Utility functions for metrics aggregation
pub mod aggregation {
    use super::*;

    /// Aggregate metrics from multiple workers
    pub struct MetricsAggregator {
        request_counts: HashMap<String, u64>,
        transaction_counts: HashMap<String, u64>,
        total_requests: u64,
        total_transactions: u64,
    }

    impl MetricsAggregator {
        pub fn new() -> Self {
            Self {
                request_counts: HashMap::new(),
                transaction_counts: HashMap::new(),
                total_requests: 0,
                total_transactions: 0,
            }
        }

        /// Add a metrics batch to the aggregator
        pub fn add_batch(&mut self, batch: &MetricsBatch) {
            // Aggregate request metrics
            for request in &batch.request_metrics {
                *self.request_counts.entry(request.name.clone()).or_insert(0) += 1;
                self.total_requests += 1;
            }

            // Aggregate transaction metrics
            for transaction in &batch.transaction_metrics {
                *self
                    .transaction_counts
                    .entry(transaction.name.clone())
                    .or_insert(0) += 1;
                self.total_transactions += 1;
            }
        }

        /// Get aggregated request counts
        pub fn get_request_counts(&self) -> &HashMap<String, u64> {
            &self.request_counts
        }

        /// Get aggregated transaction counts  
        pub fn get_transaction_counts(&self) -> &HashMap<String, u64> {
            &self.transaction_counts
        }

        /// Get total request count
        pub fn total_requests(&self) -> u64 {
            self.total_requests
        }

        /// Get total transaction count
        pub fn total_transactions(&self) -> u64 {
            self.total_transactions
        }
    }

    impl Default for MetricsAggregator {
        fn default() -> Self {
            Self::new()
        }
    }
}
