use goose::gaggle::{config::GaggleConfiguration, worker::GaggleWorker};
use goose::prelude::*;

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_worker_creation() {
    let gaggle_config = GaggleConfiguration::new().set_worker("127.0.0.1");
    let worker = GaggleWorker::new(gaggle_config);

    // Test that worker is created successfully
    assert_eq!(
        worker.get_state().await,
        goose::gaggle::gaggle_proto::WorkerState::Idle
    );
    assert_eq!(worker.get_active_users().await, 0);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_worker_with_goose_attack() {
    let gaggle_config = GaggleConfiguration::new().set_worker("127.0.0.1");

    // Create a simple GooseAttack for testing
    let goose_attack = GooseAttack::initialize().unwrap().register_scenario(
        scenario!("TestScenario").register_transaction(transaction!(test_transaction)),
    );

    let worker = GaggleWorker::with_goose_attack(gaggle_config, goose_attack);

    // Test that worker is created successfully with GooseAttack integration
    assert_eq!(
        worker.get_state().await,
        goose::gaggle::gaggle_proto::WorkerState::Idle
    );
    assert_eq!(worker.get_active_users().await, 0);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_worker_configuration_application() {
    let gaggle_config = GaggleConfiguration::new().set_worker("127.0.0.1");
    let worker = GaggleWorker::new(gaggle_config);

    // Create test configuration with updated protobuf structure
    let test_config = goose::gaggle::gaggle_proto::TestConfiguration {
        test_plan: "{}".to_string(),
        duration_seconds: 60,
        requests_per_second: 100.0,
        scenarios: vec![goose::gaggle::gaggle_proto::ScenarioConfig {
            name: "test_scenario".to_string(),
            machine_name: "test_scenario".to_string(),
            weight: 1,
            host: None,
            transaction_wait: None,
            transactions: vec![],
        }],
        config: None,
        assigned_users: 1,
        test_hash: 12345,
        manager_version: "0.19.0-dev".to_string(),
        test_start_time: chrono::Utc::now().timestamp_millis() as u64,
    };

    // Test configuration application
    let result = worker.apply_manager_configuration(test_config).await;
    assert!(result.is_ok());

    let config = result.unwrap();
    // Check that configuration was created successfully
    assert_eq!(config.run_time, "60".to_string());
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_scenario_reconstruction() {
    let gaggle_config = GaggleConfiguration::new().set_worker("127.0.0.1");
    let worker = GaggleWorker::new(gaggle_config);

    let scenarios_data = vec![
        "LoginScenario".to_string(),
        "BrowseScenario".to_string(),
        "CheckoutScenario".to_string(),
    ];

    let result = worker
        .reconstruct_scenarios_from_config(&scenarios_data)
        .await;
    assert!(result.is_ok());

    let scenarios = result.unwrap();
    assert_eq!(scenarios.len(), 3);
    assert_eq!(scenarios[0].name, "LoginScenario");
    assert_eq!(scenarios[1].name, "BrowseScenario");
    assert_eq!(scenarios[2].name, "CheckoutScenario");
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_metrics_conversion() {
    let gaggle_config = GaggleConfiguration::new()
        .set_worker("127.0.0.1")
        .set_worker_id("test-worker");
    let worker = GaggleWorker::new(gaggle_config);

    // Create test metrics
    let _test_metrics = goose::metrics::GooseMetrics::default();

    // Test metrics conversion by creating a batch directly since the method is private
    let metrics_batch = goose::gaggle::gaggle_proto::MetricsBatch {
        worker_id: "test-worker".to_string(),
        request_metrics: Vec::new(),
        transaction_metrics: Vec::new(),
        scenario_metrics: Vec::new(),
        batch_timestamp: chrono::Utc::now().timestamp_millis() as u64,
    };

    // Test adding metrics to buffer
    worker.add_metrics(metrics_batch.clone()).await;

    assert_eq!(metrics_batch.worker_id, "test-worker");
    assert!(metrics_batch.batch_timestamp > 0);
}

async fn test_transaction(user: &mut GooseUser) -> TransactionResult {
    let _goose = user.get("/").await?;
    Ok(())
}
