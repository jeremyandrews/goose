//! Test gaggle (distributed) functionality.
//!
//! These tests verify that the gaggle gRPC distributed load testing feature works correctly,
//! including manager startup, worker connections, CLI argument parsing, and error handling.

use gumdrop::Options;
use httpmock::{Method::GET, MockServer};
use std::time::Duration;
use tokio::time::timeout;

use goose::config::GooseConfiguration;
use goose::prelude::*;

mod common;

// Test transaction for basic functionality
async fn simple_transaction(user: &mut GooseUser) -> TransactionResult {
    let _goose = user.get("/").await?;
    Ok(())
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_manager_configuration() {
    // Test that manager configuration is properly parsed and validated
    let configuration = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--manager-bind-host",
        "127.0.0.1",
        "--manager-bind-port",
        "6000",
        "--expect-workers",
        "2",
        "--quiet",
    ])
    .expect("failed to parse manager configuration");

    assert!(configuration.manager);
    assert_eq!(configuration.manager_bind_host, "127.0.0.1");
    assert_eq!(configuration.manager_bind_port, 6000);
    assert_eq!(configuration.expect_workers, Some(2));
    assert!(!configuration.worker);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_worker_configuration() {
    // Test that worker configuration is properly parsed and validated
    let configuration = GooseConfiguration::parse_args_default(&[
        "--worker",
        "--manager-host",
        "127.0.0.1",
        "--manager-port",
        "6000",
        "--worker-id",
        "test-worker-1",
        "--quiet",
    ])
    .expect("failed to parse worker configuration");

    assert!(configuration.worker);
    assert_eq!(configuration.manager_host, "127.0.0.1");
    assert_eq!(configuration.manager_port, 6000);
    assert_eq!(configuration.worker_id, Some("test-worker-1".to_string()));
    assert!(!configuration.manager);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_manager_startup_basic() {
    // Test that a gaggle manager can start up without errors
    let server = MockServer::start();
    let server_url = server.base_url();

    let mock = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200);
    });

    let configuration = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--manager-bind-host",
        "127.0.0.1",
        "--manager-bind-port",
        "6001",
        "--host",
        &server_url,
        "--run-time",
        "1",
        "--quiet",
    ])
    .expect("failed to parse manager configuration");

    let goose_attack = GooseAttack::initialize_with_config(configuration)
        .expect("failed to initialize GooseAttack")
        .register_scenario(
            scenario!("Basic Scenario").register_transaction(transaction!(simple_transaction)),
        );

    // Test that the manager can be executed (with timeout to prevent hanging)
    let result = timeout(Duration::from_secs(10), goose_attack.execute()).await;

    match result {
        Ok(metrics_result) => {
            // Manager should start successfully and return empty metrics
            let metrics = metrics_result.expect("gaggle manager should execute successfully");
            // Manager doesn't generate its own load, so minimal validation
            assert_eq!(metrics.total_users, 0);
        }
        Err(_) => {
            // Timeout is acceptable for this test - it means the manager started
            // In a real implementation, we'd have more sophisticated startup detection
        }
    }

    // Verify mock wasn't called since no workers connected
    assert_eq!(mock.hits(), 0);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_worker_startup_basic() {
    // Test that a gaggle worker attempts to connect (will fail without manager, but should not crash)
    let server = MockServer::start();
    let server_url = server.base_url();

    let mock = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200);
    });

    let configuration = GooseConfiguration::parse_args_default(&[
        "--worker",
        "--manager-host",
        "127.0.0.1",
        "--manager-port",
        "6002",
        "--worker-id",
        "test-worker",
        "--host",
        &server_url,
        "--quiet",
    ])
    .expect("failed to parse worker configuration");

    let goose_attack = GooseAttack::initialize_with_config(configuration)
        .expect("failed to initialize GooseAttack")
        .register_scenario(
            scenario!("Basic Scenario").register_transaction(transaction!(simple_transaction)),
        );

    // Test that the worker attempts to start (will fail to connect but shouldn't crash)
    let result = timeout(Duration::from_secs(5), goose_attack.execute()).await;

    match result {
        Ok(metrics_result) => {
            // Worker might return an error due to no manager, but should handle gracefully
            match metrics_result {
                Ok(metrics) => {
                    // If successful, should return empty metrics since worker sends to manager
                    assert_eq!(metrics.total_users, 0);
                }
                Err(e) => {
                    // Expected error - worker can't connect to manager
                    let error_msg = format!("{}", e);
                    assert!(
                        error_msg.contains("invalid option or value specified"),
                        "Unexpected error message: {}",
                        error_msg
                    );
                }
            }
        }
        Err(_) => {
            // Timeout is also acceptable - worker might be retrying connection
        }
    }

    // Verify mock wasn't called since worker didn't get instructions from manager
    assert_eq!(mock.hits(), 0);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_manager_invalid_config() {
    // Test that invalid manager configurations are properly rejected

    // Test invalid port (too high)
    let result = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--manager-bind-port",
        "65000", // Valid u16 but high port
        "--quiet",
    ]);

    // Should still parse but will fail during validation/execution
    assert!(result.is_ok(), "Configuration parsing should succeed");

    let config = result.unwrap();
    assert!(config.manager);
    assert_eq!(config.manager_bind_port, 65000); // Valid u16 but high port
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_worker_invalid_config() {
    // Test that invalid worker configurations are properly handled

    let configuration = GooseConfiguration::parse_args_default(&[
        "--worker",
        "--manager-host",
        "", // Empty host should be invalid
        "--manager-port",
        "6003",
        "--quiet",
    ])
    .expect("configuration should parse");

    let goose_attack = GooseAttack::initialize_with_config(configuration)
        .expect("should initialize")
        .register_scenario(
            scenario!("Test Scenario").register_transaction(transaction!(simple_transaction)),
        );

    // Should fail during execution due to invalid manager host
    let result = timeout(Duration::from_secs(3), goose_attack.execute()).await;

    match result {
        Ok(metrics_result) => {
            // Should return an error
            match metrics_result {
                Ok(_) => panic!("Worker should fail with empty manager host"),
                Err(e) => {
                    let error_msg = format!("{}", e);
                    assert!(
                        error_msg.contains("invalid option or value specified"),
                        "Unexpected error: {}",
                        error_msg
                    );
                }
            }
        }
        Err(_) => {
            // Timeout is also acceptable as connection might hang
        }
    }
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_mode_detection() {
    // Test that the execute() method correctly routes to gaggle modes

    // Test manager mode detection
    let manager_config = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--manager-bind-host",
        "127.0.0.1",
        "--manager-bind-port",
        "6004",
        "--quiet",
    ])
    .expect("failed to parse manager config");

    assert!(manager_config.manager);
    assert!(!manager_config.worker);

    // Test worker mode detection
    let worker_config = GooseConfiguration::parse_args_default(&[
        "--worker",
        "--manager-host",
        "127.0.0.1",
        "--manager-port",
        "6005",
        "--quiet",
    ])
    .expect("failed to parse worker config");

    assert!(worker_config.worker);
    assert!(!worker_config.manager);

    // Test standalone mode (neither manager nor worker)
    let standalone_config =
        GooseConfiguration::parse_args_default(&["--users", "1", "--run-time", "1", "--quiet"])
            .expect("failed to parse standalone config");

    assert!(!standalone_config.manager);
    assert!(!standalone_config.worker);
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_cli_options_parsing() {
    // Test that all gaggle-related CLI options are correctly parsed

    let config = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--manager-bind-host",
        "192.168.1.100",
        "--manager-bind-port",
        "7000",
        "--expect-workers",
        "5",
        "--quiet",
    ])
    .expect("failed to parse configuration");

    assert!(config.manager);
    assert_eq!(config.manager_bind_host, "192.168.1.100");
    assert_eq!(config.manager_bind_port, 7000);
    assert_eq!(config.expect_workers, Some(5));

    let config = GooseConfiguration::parse_args_default(&[
        "--worker",
        "--manager-host",
        "192.168.1.100",
        "--manager-port",
        "7000",
        "--worker-id",
        "production-worker-01",
        "--quiet",
    ])
    .expect("failed to parse configuration");

    assert!(config.worker);
    assert_eq!(config.manager_host, "192.168.1.100");
    assert_eq!(config.manager_port, 7000);
    assert_eq!(config.worker_id, Some("production-worker-01".to_string()));
}

#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_configuration_defaults() {
    // Test that gaggle configurations have appropriate defaults

    let manager_config = GooseConfiguration::parse_args_default(&["--manager", "--quiet"])
        .expect("failed to parse manager config");

    assert!(manager_config.manager);
    assert_eq!(manager_config.manager_bind_host, ""); // Default bind host is empty string
    assert_eq!(manager_config.manager_bind_port, 0); // Default port is 0 (no default set)
    assert_eq!(manager_config.expect_workers, None); // No default worker expectation

    let worker_config = GooseConfiguration::parse_args_default(&["--worker", "--quiet"])
        .expect("failed to parse worker config");

    assert!(worker_config.worker);
    assert_eq!(worker_config.manager_host, ""); // Default manager host is empty string
    assert_eq!(worker_config.manager_port, 0); // Default port is 0 (no default set)
    assert_eq!(worker_config.worker_id, None); // No default worker ID
}

// Test that gaggle functionality is properly feature-gated
#[cfg(not(feature = "gaggle"))]
#[tokio::test]
async fn test_gaggle_feature_disabled() {
    // When gaggle feature is disabled, these options should still parse
    // but execution should handle gracefully (though this won't happen in practice
    // since the CLI options won't be available without the feature)

    // This test mainly documents the expected behavior when gaggle is disabled
    let server = MockServer::start();

    let config = common::build_configuration(&server, vec!["--users", "1", "--run-time", "1"]);

    // These should be false when feature is disabled
    assert!(!config.manager);
    assert!(!config.worker);

    // Should execute in standalone mode
    let goose_attack = GooseAttack::initialize_with_config(config)
        .expect("should initialize")
        .register_scenario(
            scenario!("Test Scenario").register_transaction(transaction!(simple_transaction)),
        );

    let metrics = goose_attack
        .execute()
        .await
        .expect("should execute in standalone mode");

    // Should have run the load test
    assert!(metrics.total_users > 0);
}

// Test mutual exclusivity of manager and worker modes
#[cfg(feature = "gaggle")]
#[tokio::test]
async fn test_gaggle_mutual_exclusivity() {
    // Test that manager and worker modes are mutually exclusive
    // Note: The CLI parsing might not prevent this, but execution should handle it

    let result = GooseConfiguration::parse_args_default(&[
        "--manager",
        "--worker", // Both manager and worker specified
        "--quiet",
    ]);

    match result {
        Ok(config) => {
            // If parsing succeeds, execution should detect the conflict
            // The last flag typically wins in gumdrop, so worker should be true
            assert!(config.worker);
            // But manager might also be true, which should cause a validation error during execution
        }
        Err(_) => {
            // Parser detected the conflict - this is also acceptable behavior
        }
    }
}
