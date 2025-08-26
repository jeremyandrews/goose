//! Gaggle module for distributed load testing
//!
//! This module provides Manager and Worker implementations for distributed
//! load testing using gRPC for communication between nodes.
//!
//! # Features
//!
//! This module is only available when the `gaggle` feature is enabled.

#[cfg(feature = "gaggle")]
pub mod config;
#[cfg(feature = "gaggle")]
pub mod manager;
#[cfg(feature = "gaggle")]
pub mod metrics;
#[cfg(feature = "gaggle")]
pub mod worker;

#[cfg(feature = "gaggle")]
pub use config::GaggleConfiguration;
#[cfg(feature = "gaggle")]
pub use manager::GaggleManager;
#[cfg(feature = "gaggle")]
pub use worker::GaggleWorker;

// Re-export generated protobuf types
#[cfg(feature = "gaggle")]
pub use gaggle_proto::*;

#[cfg(feature = "gaggle")]
mod gaggle_proto {
    include!(concat!(env!("OUT_DIR"), "/gaggle.rs"));
}

// Generated gRPC service definitions will be included from build output
// No need for manual service definitions since tonic-build handles this

// Provide helpful error messages when gaggle features are used without the feature flag
#[cfg(not(feature = "gaggle"))]
pub fn gaggle_not_available() -> ! {
    panic!(
        "Gaggle functionality is not available. Please compile with the 'gaggle' feature enabled:\n\
         cargo build --features gaggle\n\
         or\n\
         cargo run --features gaggle"
    );
}

// Stub types for when gaggle feature is disabled
#[cfg(not(feature = "gaggle"))]
pub struct GaggleManager;

#[cfg(not(feature = "gaggle"))]
pub struct GaggleWorker;

#[cfg(not(feature = "gaggle"))]
pub struct GaggleConfiguration;

#[cfg(not(feature = "gaggle"))]
impl GaggleManager {
    pub fn new(_config: &GaggleConfiguration) -> Self {
        gaggle_not_available()
    }
}

#[cfg(not(feature = "gaggle"))]
impl GaggleWorker {
    pub fn new(_config: &GaggleConfiguration) -> Self {
        gaggle_not_available()
    }
}

#[cfg(not(feature = "gaggle"))]
impl GaggleConfiguration {
    pub fn new() -> Self {
        gaggle_not_available()
    }
}
