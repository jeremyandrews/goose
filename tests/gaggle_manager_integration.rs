//! Test file for GaggleManager GooseAttack integration

use goose::prelude::*;

#[cfg(feature = "gaggle")]
mod gaggle_tests {
    use super::*;
    use goose::gaggle::{config::GaggleConfiguration, manager::GaggleManager};

    #[test]
    fn test_gaggle_manager_basic_creation() {
        // Test basic manager creation without GooseAttack
        let gaggle_config = GaggleConfiguration::new().set_manager();
        let manager = GaggleManager::new(gaggle_config);

        assert!(!manager.has_goose_attack());
    }

    #[test]
    fn test_gaggle_manager_with_goose_attack() {
        // Create a simple GooseAttack instance
        let mut goose_attack = GooseAttack::initialize().unwrap();

        // Add a simple scenario for testing
        goose_attack = goose_attack.register_scenario(
            scenario!("test_scenario").register_transaction(transaction!(test_transaction)),
        );

        // Create gaggle configuration
        let gaggle_config = GaggleConfiguration::new().set_manager();

        // Test manager creation with GooseAttack
        let manager = GaggleManager::with_goose_attack(gaggle_config, goose_attack);

        assert!(manager.has_goose_attack());

        // Test configuration synchronization
        let sync_result = manager.sync_configuration();
        assert!(sync_result.is_ok());

        // Test test plan extraction
        let test_plan_result = manager.get_test_plan();
        assert!(test_plan_result.is_ok());
        let test_plan = test_plan_result.unwrap();
        assert!(!test_plan.is_empty());

        // Verify the test plan contains scenario information
        let test_plan_str = String::from_utf8(test_plan).unwrap();
        assert!(test_plan_str.contains("scenarios_count"));
    }

    async fn test_transaction(user: &mut GooseUser) -> TransactionResult {
        // Simple test transaction
        Ok(())
    }

    #[tokio::test]
    async fn test_gaggle_manager_async_operations() {
        // Create a simple GooseAttack instance
        let mut goose_attack = GooseAttack::initialize().unwrap();
        goose_attack = goose_attack.register_scenario(
            scenario!("async_test_scenario").register_transaction(transaction!(test_transaction)),
        );

        let gaggle_config = GaggleConfiguration::new().set_manager();
        let manager = GaggleManager::with_goose_attack(gaggle_config, goose_attack);

        // Test async operations
        assert_eq!(manager.worker_count().await, 0);

        let workers = manager.get_workers().await;
        assert!(workers.is_empty());

        let metrics = manager.get_aggregated_metrics().await;
        assert!(metrics.is_empty());

        // Test configuration distribution
        let distribute_result = manager.distribute_test_setup().await;
        assert!(distribute_result.is_ok());
    }
}
