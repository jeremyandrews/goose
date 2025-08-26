# Gaggle gRPC Implementation Plan

## Overview

This document outlines a comprehensive multi-phase plan to restore Goose Gaggle (distributed load testing) functionality using gRPC/Tonic, based on the detailed analysis and requirements from [Issue #645](https://github.com/tag1consulting/goose/issues/645).

## Background and Context

### Current State Analysis
- **Gaggle functionality completely removed from Goose** (no longer available)
- Users needing distributed testing must use alternative tools or older Goose versions
- Clean slate implementation opportunity with modern technology

### Business Impact
- Users cannot perform distributed load testing with current Goose
- Goose lacks competitive distributed testing capabilities
- Market opportunity to restore this functionality with modern implementation

## Solution: gRPC/Tonic Implementation

**gRPC/Tonic** was selected as the replacement solution for the following reasons:

1. **Architectural Alignment**: Manager-Worker pattern maps directly to gRPC client-server
2. **Performance**: Binary protocol buffers significantly more efficient than JSON/CBOR
3. **Production Ready**: Battle-tested in distributed systems at scale
4. **Ecosystem Maturity**: Excellent Rust support through Tonic
5. **Maintainability**: Industry-standard patterns, code generation
6. **Security**: Built-in TLS, authentication, health checking

## Functional Requirements (From Issue #645)

### Core Functionality
- [x] **Manager Mode**: Coordinate multiple workers, aggregate metrics
- [x] **Worker Mode**: Execute load tests, stream metrics to manager
- [x] **Binary Validation**: Hash-based verification of identical test plans
- [x] **Real-time Metrics**: Continuous metric aggregation during test execution
- [x] **Failure Recovery**: Handle worker disconnections gracefully
- [x] **Configuration Compatibility**: Restore CLI flags and configuration options
- [x] **Performance Parity**: No regression from nng-based implementation

### Success Criteria
**Functional Requirements:**
- [ ] Manager can coordinate multiple workers
- [ ] Workers stream metrics in real-time
- [ ] Binary hash validation prevents version mismatches
- [ ] Graceful handling of worker failures
- [ ] Modern CLI interface for distributed testing

**Performance Requirements:**
- [ ] Efficient load testing performance
- [ ] Manager supports ≥100 concurrent workers
- [ ] Metrics latency <10ms p99
- [ ] Memory usage <5MB per worker connection

**Quality Requirements:**
- [ ] >90% test coverage for gaggle module
- [ ] Zero build-time dependencies
- [ ] Comprehensive error handling
- [ ] Complete documentation and examples

## Multi-Phase Implementation Plan

### Phase 1: Foundation and Protocol Definition
**Goal**: Establish gRPC infrastructure and protocol

#### Technical Architecture (Enhanced from Issue #645)
```
Manager (gRPC Server)
├── WorkerService
│   ├── RegisterWorker(WorkerInfo) -> WorkerId
│   ├── CommandStream(stream WorkerState) -> stream ManagerCommand
│   └── SubmitMetrics(stream MetricsBatch) -> MetricsResponse
└── Health Service (built-in)

Workers (gRPC Clients)
├── Connect and register with Manager
├── Maintain bi-directional command stream
├── Execute load test based on received configuration
└── Stream metrics continuously
```

#### Deliverables:
- [ ] **Protocol Buffer Definitions** (`proto/gaggle.proto`)
  ```protobuf
  syntax = "proto3";
  
  package gaggle.v1;
  
  service GaggleManager {
    rpc RegisterWorker(WorkerRegistration) returns (ManagerInformation);
    rpc SubmitMetrics(stream MetricsBatch) returns (MetricsResponse);
    rpc CommandStream(stream WorkerState) returns (stream ManagerCommand);
  }
  
  message WorkerRegistration {
    string worker_id = 1;
    string hostname = 2;
    string binary_hash = 3;     // For compatibility validation
    uint64 available_cpus = 4;
  }
  
  message ManagerCommand {
    oneof command {
      StartTest start = 1;
      StopTest stop = 2;
      UpdateUsers update_users = 3;
    }
  }
  ```

- [ ] **Cargo.toml Updates**
  ```toml
  [dependencies]
  tonic = { version = "0.12", optional = true }
  prost = { version = "0.13", optional = true }
  tokio-stream = { version = "0.1", optional = true }
  
  [build-dependencies]
  tonic-build = { version = "0.12", optional = true }
  
  [features]
  default = ["reqwest/default-tls"]
  rustls-tls = ["reqwest/rustls-tls", "tokio-tungstenite/rustls"]
  gaggle = ["tonic", "prost", "tokio-stream"]
  ```

  **Feature Flag Strategy:**
  - `gaggle` feature is **NOT** included in `default` features
  - Users must explicitly opt-in: `cargo build --features gaggle`
  - Keeps core Goose lightweight for single-machine testing
  - Reduces dependency chain and build time for basic use cases

- [ ] **Build Script** (`build.rs`)
  - Protocol buffer compilation
  - Feature-gated for gaggle functionality

- [ ] **Basic gRPC Services**
  - Manager server stub implementation
  - Worker client stub implementation
  - Connection establishment patterns

### Phase 2: Core Gaggle Logic Implementation
**Goal**: Implement distributed test execution

#### Module Structure
```
src/gaggle/
├── mod.rs              // Public API and feature gates
├── manager.rs          // Manager service implementation  
├── worker.rs           // Worker client implementation
├── protocol.rs         // gRPC protocol handling
├── config.rs           // Gaggle configuration helpers
├── metrics.rs          // Metrics aggregation
├── error.rs            // Comprehensive error types
├── health.rs           // Health monitoring
└── auth.rs             // Security and authentication
```

#### Feature Flag Implementation Strategy
```rust
// src/gaggle/mod.rs - Feature gating example
#[cfg(feature = "gaggle")]
pub mod manager;
#[cfg(feature = "gaggle")]
pub mod worker;
#[cfg(feature = "gaggle")]
pub mod protocol;

#[cfg(feature = "gaggle")]
pub use manager::GaggleManager;
#[cfg(feature = "gaggle")]
pub use worker::GaggleWorker;

// Conditional compilation for gaggle functionality
#[cfg(not(feature = "gaggle"))]
pub fn gaggle_not_enabled() -> GooseError {
    GooseError::InvalidOption {
        option: "gaggle".to_string(),
        value: "disabled".to_string(), 
        detail: "Gaggle support requires compilation with --features gaggle".to_string(),
    }
}
```

#### Deliverables:
- [ ] **Manager Implementation** (`src/gaggle/manager.rs`)
  - Worker registration and lifecycle management
  - Test configuration distribution
  - Coordinated test start/stop across workers
  - Load balancing algorithm for GooseUser distribution
  - State management with `Arc<RwLock<HashMap<String, WorkerClient>>>`

- [ ] **Worker Implementation** (`src/gaggle/worker.rs`)
  - Manager connection with automatic reconnection
  - Configuration reception and application
  - Local test execution coordination
  - Metrics streaming with batching optimization
  - Graceful shutdown handling

- [ ] **Metrics Aggregation System**
  - Real-time metrics streaming from workers
  - Manager-side aggregation and consolidation
  - Late-arriving metrics handling
  - Performance optimized batching (reduce network overhead)

- [ ] **Binary Hash Validation**
  - Goose binary compatibility checking
  - Version mismatch prevention
  - Secure hash comparison

### Phase 3: Integration with Existing Goose
**Goal**: Seamless integration with current Goose architecture

#### Deliverables:
- [ ] **GooseConfiguration Extensions** (`src/config.rs`)
  ```rust
  // Feature-gated gaggle configuration options
  #[cfg(feature = "gaggle")]
  #[derive(Options, Debug, Clone, Default, Serialize, Deserialize)]
  pub struct GaggleConfiguration {
      /// Run as distributed load test Manager
      #[options(no_short)]
      pub manager: bool,
      /// Manager bind host (default: 0.0.0.0)
      #[options(no_short, meta = "HOST")]
      pub manager_bind_host: String,
      /// Manager bind port (default: 5555)  
      #[options(no_short, meta = "PORT")]
      pub manager_bind_port: u16,
      /// Run as distributed load test Worker
      #[options(no_short)]
      pub worker: bool,
      /// Manager host to connect to
      #[options(no_short, meta = "HOST")]
      pub manager_host: String,
      /// Manager port to connect to (default: 5555)
      #[options(no_short, meta = "PORT")]
      pub manager_port: u16,
      /// Number of Workers expected to connect
      #[options(no_short, meta = "COUNT")]
      pub expect_workers: u32,
      /// Optional Worker identifier
      #[options(no_short, meta = "ID")]
      pub worker_id: Option<String>,
  }

  // Add gaggle fields to main GooseConfiguration
  #[cfg(feature = "gaggle")]
  pub gaggle: GaggleConfiguration,
  ```

  **Configuration Integration:**
  - All gaggle options are feature-gated with `#[cfg(feature = "gaggle")]`
  - CLI help only shows gaggle options when feature is enabled
  - Seamless integration with existing configuration patterns

- [ ] **CLI Integration**
  - `--manager` / `--worker` mode flags
  - `--manager-bind-host` / `--manager-bind-port` options
  - `--manager-host` / `--manager-port` for worker connections
  - `--expect-workers` configuration

- [ ] **GooseAttack Integration** (`src/lib.rs`, `src/goose.rs`)
  - `execute_gaggle_manager()` method implementation
  - `execute_gaggle_worker()` method implementation
  - Modified `execute()` with automatic mode detection
  - Backward compatibility preservation

### Phase 4: Advanced Features and Reliability
**Goal**: Production-ready reliability and security

#### Deliverables:
- [ ] **Fault Tolerance System**
  - Worker reconnection with exponential backoff
  - Manager handles worker failures gracefully
  - Automatic load rebalancing on worker join/leave
  - Test continuation with partial worker pool

- [ ] **Performance Optimizations**
  - **Metrics Batching**: Bundle metrics every few seconds vs per-request
  - **Compression**: Enable gRPC compression (Gzip) for bandwidth reduction
  - **Connection Pooling**: Efficient resource utilization
  - **Memory Management**: Optimize for high worker counts

- [ ] **Security Implementation**
  - Optional TLS encryption for gRPC communication
  - Worker authentication mechanisms
  - Secure configuration distribution
  - Certificate management

- [ ] **Monitoring and Observability**
  - Health check endpoints (Manager and Workers)
  - Connection status monitoring
  - gRPC communication performance metrics
  - Structured logging integration

### Phase 5: Testing and Validation
**Goal**: Comprehensive testing and quality assurance

#### Testing Strategy
- [ ] **Unit Tests** (`src/gaggle/tests.rs`)
  - Protocol message serialization/deserialization
  - Manager state management logic
  - Worker lifecycle scenarios
  - Load balancing algorithm validation

- [ ] **Integration Tests** (`tests/gaggle.rs`)
  - End-to-end Manager-Worker communication
  - Multi-worker coordination scenarios
  - Failure recovery testing
  - Configuration validation

- [ ] **Performance Tests**
  - Scalability testing (1, 10, 50, 100+ workers)
  - Network overhead measurement
  - Memory usage profiling
  - Latency impact vs single-node baseline

- [ ] **Chaos Testing**
  - Random worker disconnections during tests
  - Network partition scenarios
  - Manager restart with active workers
  - Resource exhaustion handling

#### Performance Validation
- [ ] Verify <10ms p99 metrics latency
- [ ] Confirm support for 100+ concurrent workers
- [ ] Validate <5MB memory per worker connection
- [ ] Benchmark distributed vs single-node performance impact

### Phase 6: Documentation and Migration
**Goal**: Complete user documentation and migration support

#### Deliverables:
- [ ] **Goose Book Updates**
  - Update `src/docs/goose-book/src/SUMMARY.md`
  - New file: `src/docs/goose-book/src/gaggle/grpc.md`
  - Remove deprecation notices from existing gaggle docs
  - Add gRPC-specific configuration guide

- [ ] **User Guide** (`GAGGLE_USER_GUIDE.md`)
  - Getting started with distributed testing
  - Step-by-step setup instructions
  - Configuration examples and best practices
  - Deployment scenarios and recommendations

- [ ] **Example Applications**
  - `examples/gaggle_simple.rs` - Basic distributed test
  - `examples/gaggle_advanced.rs` - Multi-scenario distributed test
  - Real-world deployment scenarios

- [ ] **Troubleshooting Guide**
  - Common configuration issues
  - Network connectivity problems
  - Performance tuning recommendations
  - Debugging tools and techniques

### Phase 7: Controller Integration
**Goal**: Integration with existing controller interfaces

#### Deliverables:
- [ ] **WebSocket Controller Integration**
  - Extend WebSocket controller for distributed test control
  - Real-time worker status monitoring
  - Distributed test metrics visualization

- [ ] **Telnet Controller Integration**
  - Add gaggle-specific telnet commands
  - Worker management commands
  - Distributed test status queries

- [ ] **Report Generation Integration**
  - Extend HTML reports with worker-specific breakdowns
  - Network topology visualization
  - Distributed test timeline views
  - Aggregated vs per-worker metric views

## Architecture Decisions and Rationale

### Communication Pattern: Bidirectional Streaming
**Decision**: Use bidirectional gRPC streams for real-time coordination
**Rationale**:
- Manager can push control commands instantly
- Workers stream metrics continuously without polling
- Lower latency than request-response patterns
- Natural backpressure handling

### State Management: Manager Authoritative
**Decision**: Manager holds authoritative state, workers are stateless
**Rationale**:
- Simplifies failure recovery (workers restart cleanly)
- Enables dynamic worker scaling
- Centralized decision making for load balancing
- Easier debugging and monitoring

### Protocol Versioning: Forward Compatibility
**Decision**: Versioned protocol buffers starting with `gaggle.v1`
**Rationale**:
- Future protocol changes won't break existing deployments
- Enables gradual migration strategies
- Standard industry practice for evolving APIs

## Risk Mitigation Strategy

### Technical Risks
1. **gRPC Learning Curve**: Mitigate with team training and prototyping
2. **Performance Regression**: Early benchmarking and continuous monitoring
3. **Network Reliability**: Robust retry logic and connection management
4. **State Synchronization**: Careful design and comprehensive testing

### Project Risks
1. **Scope Creep**: Maintain focus on core distributed testing functionality
2. **Over-engineering**: Avoid unnecessary complexity in initial implementation
3. **Performance Bottlenecks**: Identify and address scaling issues early
4. **Community Adoption**: Ensure clear documentation and intuitive user experience

## Implementation Strategy

### Clean Implementation Approach
1. **Fresh Start**: Build gRPC gaggle from scratch with modern patterns
2. **No Legacy Constraints**: Design optimal architecture without backwards compatibility burden
3. **Modern Configuration**: Design intuitive CLI and configuration system
4. **Performance First**: Focus on efficiency and scalability from day one

## Expected Outcomes

### Immediate Benefits
- Restore distributed load testing capability for Goose users
- Enable users to upgrade from 0.16.4 to modern Goose versions
- Eliminate cmake build dependency issues
- **Optional complexity** - users choose when to include gRPC dependencies

### Long-term Benefits  
- Industry-standard gRPC foundation enables future distributed features
- Improved maintainability through proven technology stack
- Enhanced performance and reliability over nng implementation
- Foundation for advanced features (auto-scaling, cloud integration)
- **Lightweight core** - maintains Goose's fast, minimal philosophy for single-machine testing

### Feature Flag Benefits
- **Reduced binary size**: ~2-3MB smaller binaries when gaggle feature not used
- **Faster builds**: No protobuf compilation for basic use cases
- **Cleaner dependencies**: Only ~12-15 additional crates when feature enabled
- **User choice**: Explicit opt-in to distributed testing complexity
- **Backwards compatibility**: Existing users completely unaffected

## Feature Flag Implementation Guidelines

### Build Configuration
```toml
# Basic Goose usage (most users)
cargo build

# Distributed testing usage  
cargo build --features gaggle
```

### Code Organization Patterns
```rust
// Conditional module inclusion
#[cfg(feature = "gaggle")]
pub mod gaggle;

// Conditional struct fields
pub struct GooseConfiguration {
    // ... existing fields ...
    
    #[cfg(feature = "gaggle")]
    /// Gaggle-specific configuration options
    pub gaggle: GaggleConfiguration,
}

// Conditional CLI options
impl Options for GooseConfiguration {
    fn usage() -> String {
        let mut usage = String::from("Basic options...");
        
        #[cfg(feature = "gaggle")]
        {
            usage.push_str("\n\nDistributed Testing (Gaggle):");
            usage.push_str("\n  --manager          Run as gaggle manager");
            usage.push_str("\n  --worker           Run as gaggle worker");
            // ... other gaggle options
        }
        
        usage
    }
}
```

### Error Handling Strategy
- Provide clear error messages when gaggle features used without feature flag
- Guide users to recompile with `--features gaggle`
- Graceful degradation for optional gaggle functionality

### Testing Strategy
- Test matrix: both `cargo test` and `cargo test --features gaggle`
- Ensure core functionality works identically with/without gaggle feature
- Integration tests for gaggle-specific functionality only run with feature enabled

### Documentation Updates
- README.md: Add feature flag usage instructions
- Goose Book: Document when gaggle feature is required
- CLI help: Contextual help based on enabled features

## Next Steps

1. **Create Implementation Branch**: `feature/gaggle-grpc-implementation`
2. **Update Cargo.toml**: Add optional dependencies and gaggle feature
3. **Set up Development Environment**: Protocol buffer tooling, gRPC testing
4. **Begin Phase 1**: Start with protocol definition and feature-gated service stubs
5. **Establish CI/CD Pipeline**: Test both feature combinations in CI
6. **Performance Benchmarking Framework**: Baseline measurements for comparison

---

**Status**: Planning Complete - Ready for Implementation
**Issue Reference**: [#645](https://github.com/tag1consulting/goose/issues/645)
