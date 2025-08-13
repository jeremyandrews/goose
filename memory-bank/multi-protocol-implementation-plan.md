# Multi-Protocol Implementation Plan for Goose

## Overview
This document outlines the implementation plan for adding multi-protocol support to Goose, starting with gRPC and WebSocket while demonstrating HTTP/3 and GraphQL capabilities. The architecture is designed to be extensible for future protocols (database connections, message queues, etc.).

## Current State Analysis

### Existing HTTP Support
- **HTTP/1.1**: Full support via reqwest
- **HTTP/2**: Automatic support via reqwest when server supports it
- **HTTP/3**: Available via reqwest `http3` feature flag (needs enabling)
- **GraphQL**: Supported as HTTP POST with JSON (just needs examples)

### Gaps to Address
- **gRPC**: Requires complete new implementation with tonic
- **WebSocket**: Needs connection upgrade and persistent connection management
- **Protocol Abstraction**: Need unified interface for multiple protocols

## Architecture Design

### Core Abstraction Layer

```rust
// Core trait for all protocol implementations
pub trait GooseProtocol: Send + Sync {
    type Connection: Send + Sync;
    type Request: Send;
    type Response: Send;
    type Config: Clone + Send + Sync;

    // Connection lifecycle
    async fn create_connection(config: &Self::Config) -> Result<Self::Connection, GooseError>;
    async fn is_connection_healthy(connection: &Self::Connection) -> bool;
    async fn close_connection(connection: Self::Connection) -> Result<(), GooseError>;

    // Request execution
    async fn execute_request(
        connection: &mut Self::Connection,
        request: Self::Request,
        user: &GooseUser,
    ) -> Result<GooseProtocolResponse<Self::Response>, GooseError>;
}

// Unified response wrapper
pub struct GooseProtocolResponse<T> {
    pub response: T,
    pub metrics: GooseRequestMetric,
}

// Protocol enumeration
pub enum GooseProtocolType {
    Http,
    Grpc,
    WebSocket,
    // Future: Database, Cache, MessageQueue, etc.
}
```

### Enhanced GooseUser Architecture

```rust
pub struct GooseUser {
    // Existing fields...
    
    // Protocol connections
    pub http_client: Client, // Existing reqwest client
    pub grpc_connections: HashMap<String, GrpcConnection>,
    pub websocket_connections: HashMap<String, WebSocketConnection>,
    
    // Protocol-specific configuration
    pub protocol_configs: HashMap<GooseProtocolType, Box<dyn Any + Send + Sync>>,
}

impl GooseUser {
    // Enhanced HTTP methods
    pub async fn get_http3(&mut self, path: &str) -> Result<GooseResponse, Box<TransactionError>>;
    pub async fn post_graphql(&mut self, endpoint: &str, query: &str, variables: Option<serde_json::Value>) -> Result<GooseResponse, Box<TransactionError>>;
    
    // New gRPC methods
    pub async fn grpc_unary<Req, Resp>(&mut self, service_method: &str, request: Req) -> Result<GooseGrpcResponse<Resp>, Box<TransactionError>>;
    pub async fn grpc_client_stream<Req, Resp>(&mut self, service_method: &str, requests: Vec<Req>) -> Result<GooseGrpcResponse<Resp>, Box<TransactionError>>;
    pub async fn grpc_server_stream<Req, Resp>(&mut self, service_method: &str, request: Req) -> Result<GooseGrpcStreamResponse<Resp>, Box<TransactionError>>;
    pub async fn grpc_bidi_stream<Req, Resp>(&mut self, service_method: &str, requests: Vec<Req>) -> Result<GooseGrpcStreamResponse<Resp>, Box<TransactionError>>;
    
    // New WebSocket methods
    pub async fn websocket_connect(&mut self, endpoint: &str, protocols: Option<Vec<String>>) -> Result<String, Box<TransactionError>>;
    pub async fn websocket_send_text(&mut self, connection_id: &str, message: &str) -> Result<(), Box<TransactionError>>;
    pub async fn websocket_send_binary(&mut self, connection_id: &str, data: &[u8]) -> Result<(), Box<TransactionError>>;
    pub async fn websocket_receive(&mut self, connection_id: &str, timeout: Option<Duration>) -> Result<WebSocketMessage, Box<TransactionError>>;
    pub async fn websocket_close(&mut self, connection_id: &str) -> Result<(), Box<TransactionError>>;
}
```

## Implementation Phases

### Phase 1: HTTP/3 and GraphQL Examples (Week 1-2)

**Goal**: Demonstrate existing capabilities and set up enhanced configuration.

**Tasks**:
1. **Enable HTTP/3 Feature**
   - Add `http3` feature flag to Cargo.toml
   - Update reqwest client builder to support HTTP/3
   - Add configuration option for HTTP/3 preference

2. **GraphQL Helper Functions**
   - Add `post_graphql` convenience method
   - Create GraphQL request/response types
   - Add GraphQL-specific error handling

3. **Enhanced Configuration**
   ```rust
   pub struct GooseConfiguration {
       // Existing fields...
       
       // Enhanced HTTP options
       pub prefer_http3: bool,
       pub http3_max_connections: Option<usize>,
       
       // Protocol-specific configs
       pub grpc_config: Option<GrpcConfig>,
       pub websocket_config: Option<WebSocketConfig>,
   }
   ```

4. **Examples**
   - `examples/http3_loadtest.rs` - Demonstrates HTTP/3 usage
   - `examples/graphql_loadtest.rs` - Shows GraphQL query load testing

**Deliverables**:
- Enhanced HTTP client with HTTP/3 support
- GraphQL helper functions
- Two example applications
- Updated documentation in Goose Book

### Phase 2: gRPC Core Implementation (Week 3-6)

**Goal**: Add full gRPC support with all four RPC patterns.

**Tasks**:
1. **Dependencies and Feature Flags**
   ```toml
   [features]
   grpc = ["tonic", "prost", "tower"]
   http3 = ["reqwest/http3"]
   
   [dependencies]
   tonic = { version = "0.11", optional = true }
   prost = { version = "0.12", optional = true }
   tower = { version = "0.4", optional = true }
   ```

2. **gRPC Connection Management**
   ```rust
   pub struct GrpcConnection {
       pub channel: tonic::transport::Channel,
       pub service_name: String,
       pub endpoint: String,
   }
   
   impl GrpcConnection {
       pub async fn connect(endpoint: &str) -> Result<Self, tonic::transport::Error>;
       pub async fn unary_call<Req, Resp>(&mut self, method: &str, request: Req) -> Result<tonic::Response<Resp>, tonic::Status>;
       // ... other RPC patterns
   }
   ```

3. **Protocol-Specific Metrics**
   ```rust
   pub enum GooseProtocolMetrics {
       Http { status_code: u16 },
       Grpc { 
           status: tonic::Code,
           rpc_type: GrpcRpcType,
           message_count: Option<usize>, // For streaming
       },
       WebSocket {
           frame_type: WebSocketFrameType,
           connection_duration: Option<Duration>,
       },
   }
   
   pub enum GrpcRpcType {
       Unary,
       ClientStreaming,
       ServerStreaming,
       BidirectionalStreaming,
   }
   ```

4. **gRPC Request Builder**
   ```rust
   pub struct GooseGrpcRequest<T> {
       pub service_method: String,
       pub request: T,
       pub timeout: Option<Duration>,
       pub metadata: Option<tonic::metadata::MetadataMap>,
   }
   
   impl<T> GooseGrpcRequest<T> {
       pub fn new(service_method: &str, request: T) -> Self;
       pub fn with_timeout(mut self, timeout: Duration) -> Self;
       pub fn with_metadata(mut self, metadata: tonic::metadata::MetadataMap) -> Self;
   }
   ```

5. **Examples**
   - `examples/grpc_unary.rs` - Simple unary RPC load test
   - `examples/grpc_streaming.rs` - File upload with client streaming
   - `examples/grpc_mixed.rs` - Multiple RPC patterns in one test

**Deliverables**:
- Complete gRPC client implementation
- All four RPC patterns supported
- gRPC-specific metrics and error handling
- Three comprehensive examples
- gRPC section in Goose Book

### Phase 3: WebSocket Implementation (Week 7-9)

**Goal**: Add persistent WebSocket connection support.

**Tasks**:
1. **WebSocket Dependencies**
   ```toml
   tokio-tungstenite = { version = "0.20", optional = true }
   ```

2. **WebSocket Connection Management**
   ```rust
   pub struct WebSocketConnection {
       pub websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
       pub endpoint: String,
       pub connection_id: String,
       pub connected_at: Instant,
   }
   
   impl WebSocketConnection {
       pub async fn connect(url: &str, protocols: Option<Vec<String>>) -> Result<Self, Box<dyn std::error::Error>>;
       pub async fn send(&mut self, message: Message) -> Result<(), Box<dyn std::error::Error>>;
       pub async fn receive(&mut self, timeout: Option<Duration>) -> Result<Message, Box<dyn std::error::Error>>;
       pub async fn close(mut self) -> Result<(), Box<dyn std::error::Error>>;
   }
   ```

3. **WebSocket Message Types**
   ```rust
   pub enum WebSocketMessage {
       Text(String),
       Binary(Vec<u8>),
       Ping(Vec<u8>),
       Pong(Vec<u8>),
       Close(Option<CloseFrame>),
   }
   
   pub enum WebSocketFrameType {
       Text,
       Binary,
       Ping,
       Pong,
       Close,
   }
   ```

4. **Connection Lifecycle Management**
   - Connection pooling for WebSocket connections
   - Automatic reconnection on connection loss
   - Connection health monitoring
   - Graceful connection shutdown

5. **Examples**
   - `examples/websocket_chat.rs` - Chat application load test
   - `examples/websocket_realtime.rs` - Real-time data streaming
   - `examples/websocket_gaming.rs` - Gaming scenario simulation

**Deliverables**:
- Full WebSocket client implementation
- Connection lifecycle management
- WebSocket-specific metrics
- Three example applications
- WebSocket documentation section

### Phase 4: Integration and Testing (Week 10-11)

**Goal**: Integrate all protocols and ensure they work together seamlessly.

**Tasks**:
1. **Mixed Protocol Examples**
   ```rust
   // Example: E-commerce load test with multiple protocols
   async fn ecommerce_scenario(user: &mut GooseUser) -> TransactionResult {
       // HTTP: Load product page
       let product = user.get("/api/products/123").await?;
       
       // GraphQL: Get user recommendations
       let query = r#"query GetRecommendations($userId: ID!) {
           recommendations(userId: $userId) { id name price }
       }"#;
       let recommendations = user.post_graphql("/graphql", query, Some(json!({ "userId": "123" }))).await?;
       
       // WebSocket: Real-time price updates
       user.websocket_connect("wss://prices.example.com/stream").await?;
       user.websocket_send_text("price_stream", r#"{"subscribe": ["product:123"]}"#).await?;
       let price_update = user.websocket_receive("price_stream", Some(Duration::from_secs(5))).await?;
       
       // gRPC: Process order
       let order_request = CreateOrderRequest { user_id: 123, product_id: 123, quantity: 1 };
       let order_response = user.grpc_unary("orders.OrderService/CreateOrder", order_request).await?;
       
       Ok(())
   }
   ```

2. **Comprehensive Testing**
   - Unit tests for each protocol implementation
   - Integration tests for mixed-protocol scenarios
   - Performance benchmarks
   - Error handling validation

3. **Documentation Updates**
   - Updated Goose Book with multi-protocol section
   - Migration guide for existing users
   - Best practices for each protocol
   - Performance tuning guide

4. **Configuration Management**
   ```rust
   // Example comprehensive configuration
   pub struct GooseConfiguration {
       // HTTP settings
       pub host: Option<String>,
       pub prefer_http3: bool,
       pub no_gzip: bool,
       
       // gRPC settings
       pub grpc_endpoints: HashMap<String, String>,
       pub grpc_proto_paths: Vec<PathBuf>,
       pub grpc_tls_config: Option<GrpcTlsConfig>,
       
       // WebSocket settings
       pub websocket_endpoints: HashMap<String, String>,
       pub websocket_protocols: Option<Vec<String>>,
       pub websocket_ping_interval: Option<Duration>,
       
       // Unified settings
       pub connection_timeout: Duration,
       pub request_timeout: Duration,
   }
   ```

**Deliverables**:
- Mixed-protocol example applications
- Comprehensive test suite
- Complete documentation
- Configuration management system

## File Structure Changes

```
src/
├── lib.rs                           # Updated with protocol exports
├── goose.rs                         # Enhanced GooseUser
├── protocols/                       # New protocol implementations
│   ├── mod.rs                      # Protocol trait definitions
│   ├── http.rs                     # Enhanced HTTP (HTTP/3, GraphQL)
│   ├── grpc.rs                     # gRPC implementation
│   ├── websocket.rs                # WebSocket implementation
│   └── future/                     # Space for future protocols
│       ├── database.rs             # Future: Database protocols
│       ├── cache.rs                # Future: Redis/Memcache
│       └── messaging.rs            # Future: MQTT/AMQP
├── metrics.rs                      # Updated with protocol-specific metrics
└── config.rs                       # Enhanced configuration

examples/
├── http3_loadtest.rs               # HTTP/3 demonstration
├── graphql_loadtest.rs             # GraphQL load testing
├── grpc_unary.rs                   # Basic gRPC unary calls
├── grpc_streaming.rs               # File upload scenario
├── grpc_mixed.rs                   # All gRPC patterns
├── websocket_chat.rs               # Chat application test
├── websocket_realtime.rs           # Real-time data streaming
├── websocket_gaming.rs             # Gaming simulation
└── mixed_protocol_ecommerce.rs     # All protocols together
```

## Metrics and Reporting Enhancements

### Protocol-Specific Metrics Collection

```rust
pub struct GooseRequestMetric {
    // Existing fields...
    
    // Protocol identification
    pub protocol: GooseProtocolType,
    pub protocol_metrics: GooseProtocolMetrics,
    
    // Enhanced timing for streaming protocols
    pub stream_metrics: Option<StreamMetrics>,
}

pub struct StreamMetrics {
    pub messages_sent: usize,
    pub messages_received: usize,
    pub first_message_time: u128,
    pub last_message_time: u128,
    pub total_stream_duration: u128,
}
```

### Reporting Enhancements

- Protocol-specific sections in HTML reports
- gRPC status code distribution
- WebSocket connection lifecycle metrics
- Stream timing analysis
- Mixed-protocol transaction correlation

## Configuration Management

### CLI Arguments Extension

```bash
# HTTP/3 and GraphQL
goose --prefer-http3 --graphql-endpoint /graphql

# gRPC configuration
goose --grpc-endpoints user=localhost:50051,orders=localhost:50052 \
      --grpc-proto-path ./protos \
      --grpc-tls-cert ./certs/client.crt

# WebSocket configuration
goose --websocket-endpoints realtime=wss://api.example.com/ws \
      --websocket-protocols chat,v1

# Mixed protocol configuration
goose --config mixed_protocol_config.toml
```

### Configuration File Support

```toml
[http]
host = "https://api.example.com"
prefer_http3 = true
timeout = 60

[grpc]
endpoints = { user = "https://user-service:50051", orders = "https://orders-service:50052" }
proto_paths = ["./protos"]
tls_enabled = true

[websocket]
endpoints = { realtime = "wss://api.example.com/ws" }
protocols = ["chat", "v1"]
ping_interval = 30

[general]
users = 100
duration = "5m"
```

## Future Protocol Extension Points

The architecture is designed to easily accommodate additional protocols:

### Database Protocols
```rust
pub struct DatabaseConnection {
    pub connection: sqlx::AnyConnection,
    pub pool: sqlx::AnyPool,
}

impl GooseProtocol for DatabaseProtocol {
    // Connection pooling, query execution, transaction handling
}
```

### Message Queue Protocols
```rust
pub struct MessageQueueConnection {
    pub producer: kafka::Producer,
    pub consumer: kafka::Consumer,
}

impl GooseProtocol for MessageQueueProtocol {
    // Pub/sub patterns, message acknowledgments
}
```

### Cache Protocols
```rust
pub struct CacheConnection {
    pub redis_client: redis::Client,
    pub memcache_client: memcache::Client,
}

impl GooseProtocol for CacheProtocol {
    // Get/set operations, pipeline commands
}
```

## Testing Strategy

### Unit Tests
- Individual protocol implementations
- Connection lifecycle management
- Error handling for each protocol
- Metrics collection accuracy

### Integration Tests
- Mixed-protocol scenarios
- Connection pooling behavior
- Configuration parsing
- Backward compatibility

### Performance Tests
- Protocol overhead comparison
- Connection reuse efficiency
- Memory usage under load
- Concurrent connection limits

### End-to-End Tests
- Real server interactions for each protocol
- Authentication and TLS handling
- Network failure simulation
- Graceful degradation

## Risk Mitigation

### Backward Compatibility
- All existing HTTP functionality remains unchanged
- New features are opt-in via feature flags
- Configuration remains backward compatible
- Existing examples continue to work

### Performance Considerations
- Protocol-specific connection pooling
- Lazy initialization of unused protocols
- Memory-efficient connection management
- Minimal overhead for single-protocol users

### Error Handling
- Protocol-specific error types
- Graceful fallback mechanisms
- Comprehensive error reporting
- Connection recovery strategies

## Success Criteria

### Phase 1 Success Metrics
- HTTP/3 successfully enabled and demonstrated
- GraphQL helper functions work correctly
- Existing performance is maintained
- Documentation is clear and complete

### Phase 2 Success Metrics
- All four gRPC patterns implemented correctly
- gRPC performance is competitive with dedicated tools
- Error handling covers all gRPC status codes
- Examples demonstrate real-world scenarios

### Phase 3 Success Metrics
- WebSocket connections are stable under load
- Connection lifecycle is properly managed
- Real-time scenarios work reliably
- Performance scales with connection count

### Phase 4 Success Metrics
- Mixed-protocol scenarios work seamlessly
- Configuration is intuitive and flexible
- All protocols integrate with existing Goose features
- Documentation supports migration and adoption

## Timeline Summary

- **Week 1-2**: HTTP/3 and GraphQL examples
- **Week 3-6**: Complete gRPC implementation
- **Week 7-9**: WebSocket implementation
- **Week 10-11**: Integration and testing
- **Week 12**: Documentation and release preparation

This implementation plan provides a solid foundation for multi-protocol support while maintaining Goose's performance and usability advantages.
