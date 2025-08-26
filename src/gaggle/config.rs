//! Gaggle configuration types and utilities

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Configuration for gaggle distributed load testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaggleConfiguration {
    /// Whether this instance is a manager
    pub manager: bool,
    /// Whether this instance is a worker  
    pub worker: bool,
    /// Manager address to connect to (for workers)
    pub manager_host: String,
    /// Port for manager gRPC server
    pub manager_port: u16,
    /// Worker identifier
    pub worker_id: Option<String>,
    /// Maximum number of users this worker can handle
    pub max_users: Option<u32>,
    /// List of worker capabilities
    pub capabilities: Vec<String>,
    /// Timeout for gRPC connections in seconds
    pub connection_timeout: u64,
    /// Heartbeat interval in seconds
    pub heartbeat_interval: u64,
}

impl Default for GaggleConfiguration {
    fn default() -> Self {
        Self {
            manager: false,
            worker: false,
            manager_host: "127.0.0.1".to_string(),
            manager_port: 5115,
            worker_id: None,
            max_users: None,
            capabilities: vec![],
            connection_timeout: 30,
            heartbeat_interval: 10,
        }
    }
}

impl GaggleConfiguration {
    /// Create a new gaggle configuration
    pub fn new() -> Self {
        Default::default()
    }

    /// Set this instance as a manager
    pub fn set_manager(mut self) -> Self {
        self.manager = true;
        self.worker = false;
        self
    }

    /// Set this instance as a worker
    pub fn set_worker(mut self, manager_host: impl Into<String>) -> Self {
        self.worker = true;
        self.manager = false;
        self.manager_host = manager_host.into();
        self
    }

    /// Set the worker ID
    pub fn set_worker_id(mut self, id: impl Into<String>) -> Self {
        self.worker_id = Some(id.into());
        self
    }

    /// Set the maximum number of users this worker can handle
    pub fn set_max_users(mut self, max_users: u32) -> Self {
        self.max_users = Some(max_users);
        self
    }

    /// Add a capability to this worker
    pub fn add_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Get the manager socket address
    pub fn manager_address(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.manager_host, self.manager_port).parse()
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), GaggleConfigError> {
        if self.manager && self.worker {
            return Err(GaggleConfigError::BothManagerAndWorker);
        }

        if !self.manager && !self.worker {
            return Err(GaggleConfigError::NeitherManagerNorWorker);
        }

        if self.worker && self.manager_host.is_empty() {
            return Err(GaggleConfigError::MissingManagerHost);
        }

        if self.worker && self.worker_id.is_none() {
            return Err(GaggleConfigError::MissingWorkerId);
        }

        Ok(())
    }
}

/// Errors that can occur in gaggle configuration
#[derive(Debug, thiserror::Error)]
pub enum GaggleConfigError {
    #[error("Cannot be both manager and worker")]
    BothManagerAndWorker,
    #[error("Must be either manager or worker")]
    NeitherManagerNorWorker,
    #[error("Worker must specify manager host")]
    MissingManagerHost,
    #[error("Worker must specify worker ID")]
    MissingWorkerId,
}

/// Extension trait to add gaggle configuration to GooseConfiguration
pub trait GooseGaggleExt {
    /// Get the gaggle configuration
    fn gaggle(&self) -> Option<&GaggleConfiguration>;
    /// Set the gaggle configuration
    fn set_gaggle(&mut self, gaggle: GaggleConfiguration);
}

// Note: This would be implemented on GooseConfiguration in a future phase
// when we integrate with the main configuration system
