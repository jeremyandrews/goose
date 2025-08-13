# Phase 1 Implementation Plan: HTTP/3 and GraphQL Support

## Overview

This document details the implementation of Phase 1 for multi-protocol support in Goose, focusing on HTTP/3 and GraphQL capabilities. HTTP/3 will be implemented as an ergonomic compile-time optional feature, while GraphQL will be added as helper methods using the existing HTTP infrastructure.

## Goals

- Enable HTTP/3 support as an optional compile-time feature
- Add GraphQL helper methods for common query patterns
- Create example applications demonstrating both capabilities
- Maintain backward compatibility and performance
- Set foundation for future multi-protocol work

## Current State Analysis

### Branch Status
- Working on `http3` branch - perfect for this implementation
- Current reqwest version: 0.12 with basic features (cookies, gzip, json)
- Configuration system is comprehensive and extensible
- GooseUser has robust HTTP helper methods

### Dependencies
- reqwest 0.12 supports HTTP/3 via `http3` feature flag
- Existing JSON support covers GraphQL request/response handling
- No additional external dependencies required

## Detailed Implementation Steps

### 1. Cargo.toml Changes

**File: `Cargo.toml`**

Update the reqwest dependency and add feature flags:

```toml
[dependencies]
# Keep existing reqwest configuration as base
reqwest = { version = "0.12", default-features = false, features = [
    "cookies",
    "gzip",
    "json",
] }

# ... existing dependencies remain unchanged ...

[features]
default = ["reqwest/default-tls"]
rustls-tls = ["reqwest/rustls-tls", "tokio-tungstenite/rustls"]
gaggle = []
# NEW: HTTP/3 feature - adds http3 support to reqwest
http3 = ["reqwest/http3"]
```

**Benefits of this approach:**
- Zero overhead when HTTP/3 not needed
- Standard Rust feature flag pattern
- Clean dependency management

### 2. Configuration System Updates

**File: `src/config.rs`**

Add new configuration options with conditional compilation:

```rust
#[derive(Options, Debug, Clone, Default, Serialize, Deserialize)]
pub struct GooseConfiguration {
    // ... existing fields unchanged ...

    /// Disable HTTP/3 preference (only with http3 feature, defaults to true when feature enabled)
    #[cfg(feature = "http3")]
    #[options(no_short, help = "Disable HTTP/3 preference (HTTP/3 is preferred by default when feature enabled)")]
    pub no_http3: bool,
    
    /// GraphQL endpoint path for GraphQL requests
    #[options(no_short, meta = "PATH", help = "GraphQL endpoint path (default: /graphql)")]
    pub graphql_endpoint: String,
}
```

Extend `GooseDefaults` struct:

```rust
#[derive(Clone, Debug, Default)]
pub(crate) struct GooseDefaults {
    // ... existing fields unchanged ...
    
    /// Optional default for disabling HTTP/3
    #[cfg(feature = "http3")]
    pub no_http3: Option<bool>,
    
    /// Optional default GraphQL endpoint
    pub graphql_endpoint: Option<String>,
}
```

Add to `GooseDefault` enum:

```rust
pub enum GooseDefault {
    // ... existing variants unchanged ...
    
    /// Optional default for disabling HTTP/3 preference
    #[cfg(feature = "http3")]
    NoHttp3,
    
    /// Optional default GraphQL endpoint
    GraphqlEndpoint,
}
```

Update the configuration implementation methods:

```rust
impl GooseConfiguration {
    pub(crate) fn configure(&mut self, defaults: &GooseDefaults) {
        // ... existing configuration logic ...

        // Configure HTTP/3 preference (defaults to enabled when feature is compiled in)
        #[cfg(feature = "http3")]
        {
            self.no_http3 = self
                .get_value(vec![
                    GooseValue {
                        value: Some(self.no_http3),
                        filter: !self.no_http3,
                        message: "no_http3",
                    },
                    GooseValue {
                        value: defaults.no_http3,
                        filter: defaults.no_http3.is_none(),
                        message: "no_http3",
                    },
                ])
                .unwrap_or(false); // Default to false (meaning HTTP/3 is preferred)
        }

        // Configure GraphQL endpoint
        self.graphql_endpoint = self
            .get_value(vec![
                GooseValue {
                    value: Some(self.graphql_endpoint.clone()),
                    filter: self.graphql_endpoint.is_empty(),
                    message: "graphql_endpoint",
                },
                GooseValue {
                    value: defaults.graphql_endpoint.clone(),
                    filter: defaults.graphql_endpoint.is_none(),
                    message: "graphql_endpoint",
                },
            ])
            .unwrap_or_else(|| "/graphql".to_string());
    }
}
```

Add trait implementations for new defaults:

```rust
impl GooseDefaultType<bool> for GooseAttack {
    fn set_default(mut self, key: GooseDefault, value: bool) -> Result<Box<Self>, GooseError> {
        match key {
            // ... existing matches ...
            
            #[cfg(feature = "http3")]
            GooseDefault::NoHttp3 => self.defaults.no_http3 = Some(value),
            
            // ... rest of existing matches and error handling ...
        }
        Ok(Box::new(self))
    }
}

impl GooseDefaultType<&str> for GooseAttack {
    fn set_default(mut self, key: GooseDefault, value: &str) -> Result<Box<Self>, GooseError> {
        match key {
            // ... existing matches ...
            
            GooseDefault::GraphqlEndpoint => self.defaults.graphql_endpoint = Some(value.to_string()),
            
            // ... rest of existing matches and error handling ...
        }
        Ok(Box::new(self))
    }
}
```

### 3. HTTP Client Enhancement

**File: `src/goose.rs`**

Update the `create_reqwest_client` function to support HTTP/3:

```rust
/// Internal helper function to create the default GooseUser reqwest client
pub(crate) fn create_reqwest_client(
    configuration: &GooseConfiguration,
) -> Result<Client, reqwest::Error> {
    // Existing timeout logic unchanged
    let timeout = if configuration.timeout.is_some() {
        match crate::util::get_float_from_string(configuration.timeout.clone()) {
            Some(f) => f as u64 * 1_000,
            None => GOOSE_REQUEST_TIMEOUT,
        }
    } else {
        GOOSE_REQUEST_TIMEOUT
    };

    let mut builder = Client::builder()
        .user_agent(APP_USER_AGENT)
        .cookie_store(true)
        .timeout(Duration::from_millis(timeout))
        .gzip(!configuration.no_gzip)
        .danger_accept_invalid_certs(configuration.accept_invalid_certs);

    // Enable HTTP/3 by default when feature is compiled in (unless explicitly disabled)
    #[cfg(feature = "http3")]
    {
        if !configuration.no_http3 {
            builder = builder.http3_prior_knowledge();
        }
    }

    builder.build()
}
```

### 4. GraphQL Helper Methods

**File: `src/goose.rs`**

Add GraphQL methods to the `GooseUser` implementation:

```rust
impl GooseUser {
    // ... existing methods unchanged ...

    /// A helper to make a GraphQL query request and collect relevant metrics.
    /// Automatically prepends the correct host and uses the configured GraphQL endpoint.
    ///
    /// # Example
    /// ```rust
    /// use goose::prelude::*;
    /// use serde_json::json;
    ///
    /// async fn graphql_query(user: &mut GooseUser) -> TransactionResult {
    ///     let query = r#"
    ///         query GetUsers {
    ///             users {
    ///                 id
    ///                 name
    ///                 email
    ///             }
    ///         }
    ///     "#;
    ///     
    ///     let _goose = user.post_graphql(query, None).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn post_graphql(
        &mut self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> Result<GooseResponse, Box<TransactionError>> {
        let endpoint = if self.config.graphql_endpoint.is_empty() {
            "/graphql"
        } else {
            &self.config.graphql_endpoint
        };

        let mut request_body = serde_json::json!({
            "query": query
        });

        if let Some(vars) = variables {
            request_body["variables"] = vars;
        }

        // Use existing post_json method for the actual request
        self.post_json(endpoint, &request_body).await
    }

    /// A helper to make a named GraphQL query request and collect relevant metrics.
    /// The name will appear in metrics and can be used for request identification.
    ///
    /// # Example
    /// ```rust
    /// use goose::prelude::*;
    /// use serde_json::json;
    ///
    /// async fn named_graphql_query(user: &mut GooseUser) -> TransactionResult {
    ///     let query = r#"
    ///         query GetUserPosts($userId: ID!) {
    ///             user(id: $userId) {
    ///                 posts {
    ///                     id
    ///                     title
    ///                 }
    ///             }
    ///         }
    ///     "#;
    ///     
    ///     let variables = json!({"userId": "123"});
    ///     let _goose = user.post_graphql_named(query, Some(variables), "GetUserPosts").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn post_graphql_named(
        &mut self,
        query: &str,
        variables: Option<serde_json::Value>,
        name: &str,
    ) -> Result<GooseResponse, Box<TransactionError>> {
        let endpoint = if self.config.graphql_endpoint.is_empty() {
            "/graphql"
        } else {
            &self.config.graphql_endpoint
        };

        let mut request_body = serde_json::json!({
            "query": query
        });

        if let Some(vars) = variables {
            request_body["variables"] = vars;
        }

        // Build the request manually to set a custom name
        let goose_request = GooseRequest::builder()
            .method(GooseMethod::Post)
            .path(endpoint)
            .name(name)
            .set_request_builder(
                self.get_request_builder(&GooseMethod::Post, endpoint)?
                    .json(&request_body)
            )
            .build();

        self.request(goose_request).await
    }
}
```

### 5. Example Applications

**File: `examples/http3_loadtest.rs`**

```rust
//! # HTTP/3 Load Testing Example
//!
//! This example demonstrates HTTP/3 load testing capabilities in Goose.
//! 
//! ## Requirements
//! 
//! This example requires the `http3` feature to be enabled:
//! 
//! ```bash
//! cargo run --example http3_loadtest --features http3
//! ```
//! 
//! ## HTTP/3 Configuration
//! 
//! HTTP/3 is enabled by default when the feature is compiled in.
//! It can be disabled via:
//! - Command line: `--no-http3`
//! - Programmatically: `.set_default(GooseDefault::NoHttp3, true)`

#[cfg(feature = "http3")]
use goose::prelude::*;
#[cfg(feature = "http3")]
use std::time::Duration;

#[cfg(feature = "http3")]
#[tokio::main]
async fn main() -> Result<(), GooseError> {
    println!("Starting HTTP/3 Load Test");
    println!("This test will prefer HTTP/3 connections when available");

    GooseAttack::initialize()?
        .register_scenario(
            scenario!("HTTP3LoadTest")
                .set_host("https://http3.example.com")
                .set_wait_time(Duration::from_secs(1), Duration::from_secs(3))?
                .register_transaction(transaction!(http3_get_request).set_weight(3)?)
                .register_transaction(transaction!(http3_post_request).set_weight(1)?)
        )
        // HTTP/3 is preferred by default when feature is enabled
        // Uncomment below to disable HTTP/3 if needed:
        // .set_default(GooseDefault::NoHttp3, true)?
        .set_default(GooseDefault::Users, 10)?
        .set_default(GooseDefault::RunTime, 30)?
        .set_default(GooseDefault::LogLevel, 1)?
        .execute()
        .await?;

    Ok(())
}

#[cfg(feature = "http3")]
async fn http3_get_request(user: &mut GooseUser) -> TransactionResult {
    // Standard GET request - will use HTTP/3 if server supports it and it's enabled
    let _goose = user.get("/api/data").await?;
    Ok(())
}

#[cfg(feature = "http3")]
async fn http3_post_request(user: &mut GooseUser) -> TransactionResult {
    // POST request with JSON payload
    let payload = serde_json::json!({
        "action": "test",
        "timestamp": chrono::Utc::now().timestamp(),
        "data": "HTTP/3 load test"
    });
    
    let _goose = user.post_json("/api/submit", &payload).await?;
    Ok(())
}

#[cfg(not(feature = "http3"))]
fn main() {
    eprintln!("❌ This example requires the 'http3' feature to be enabled.");
    eprintln!("📖 Run with: cargo run --example http3_loadtest --features http3");
    eprintln!("💡 Or add 'http3' to your default features in Cargo.toml");
    std::process::exit(1);
}
```

**File: `examples/graphql_loadtest.rs`**

```rust
//! # GraphQL Load Testing Example
//!
//! This example demonstrates GraphQL load testing capabilities in Goose.
//! It shows how to use the GraphQL helper methods for common query patterns.
//!
//! ## Running the Example
//!
//! ```bash
//! cargo run --example graphql_loadtest
//! ```
//!
//! ## GraphQL Configuration
//!
//! GraphQL endpoint can be configured via:
//! - Command line: `--graphql-endpoint /api/graphql`
//! - Programmatically: `.set_default(GooseDefault::GraphqlEndpoint, "/api/graphql")`

use goose::prelude::*;
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), GooseError> {
    println!("Starting GraphQL Load Test");
    
    GooseAttack::initialize()?
        .register_scenario(
            scenario!("GraphQLLoadTest")
                .set_host("https://api.example.com")
                .set_wait_time(Duration::from_millis(500), Duration::from_secs(2))?
                .register_transaction(transaction!(simple_query).set_weight(5)?)
                .register_transaction(transaction!(query_with_variables).set_weight(3)?)
                .register_transaction(transaction!(complex_nested_query).set_weight(2)?)
                .register_transaction(transaction!(mutation_example).set_weight(1)?)
        )
        // Configure GraphQL endpoint (default is "/graphql")
        .set_default(GooseDefault::GraphqlEndpoint, "/api/graphql")?
        .set_default(GooseDefault::Users, 5)?
        .set_default(GooseDefault::RunTime, 60)?
        .execute()
        .await?;

    Ok(())
}

async fn simple_query(user: &mut GooseUser) -> TransactionResult {
    let query = r#"
        query GetUsers {
            users {
                id
                name
                email
                createdAt
            }
        }
    "#;

    let _goose = user.post_graphql_named(query, None, "GetUsers").await?;
    Ok(())
}

async fn query_with_variables(user: &mut GooseUser) -> TransactionResult {
    let query = r#"
        query GetUserPosts($userId: ID!, $limit: Int) {
            user(id: $userId) {
                name
                posts(limit: $limit) {
                    id
                    title
                    content
                    publishedAt
                    author {
                        name
                    }
                }
            }
        }
    "#;

    // Use a random user ID for more realistic load testing
    let user_id = rand::random::<u32>() % 1000 + 1;
    let variables = json!({
        "userId": user_id.to_string(),
        "limit": 10
    });

    let _goose = user.post_graphql_named(query, Some(variables), "GetUserPosts").await?;
    Ok(())
}

async fn complex_nested_query(user: &mut GooseUser) -> TransactionResult {
    let query = r#"
        query GetUserProfile($userId: ID!) {
            user(id: $userId) {
                id
                name
                email
                profile {
                    bio
                    avatar
                    preferences {
                        theme
                        notifications
                    }
                }
                posts(limit: 5) {
                    id
                    title
                    excerpt
                    tags {
                        name
                        color
                    }
                    comments(limit: 3) {
                        id
                        content
                        author {
                            name
                        }
                    }
                }
                following {
                    id
                    name
                }
                followers {
                    id
                    name
                }
            }
        }
    "#;

    let user_id = rand::random::<u32>() % 100 + 1;
    let variables = json!({
        "userId": user_id.to_string()
    });

    let _goose = user.post_graphql_named(query, Some(variables), "GetUserProfile").await?;
    Ok(())
}

async fn mutation_example(user: &mut GooseUser) -> TransactionResult {
    let mutation = r#"
        mutation CreatePost($input: CreatePostInput!) {
            createPost(input: $input) {
                id
                title
                content
                publishedAt
                author {
                    id
                    name
                }
            }
        }
    "#;

    let variables = json!({
        "input": {
            "title": format!("Load Test Post {}", rand::random::<u32>()),
            "content": "This post was created during a Goose load test to validate GraphQL mutation performance.",
            "tags": ["loadtest", "goose", "graphql"]
        }
    });

    let _goose = user.post_graphql_named(mutation, Some(variables), "CreatePost").await?;
    Ok(())
}
```

### 6. Documentation Updates

#### Goose Book Updates

**File: `src/docs/goose-book/src/SUMMARY.md`**

Add new sections to the table of contents:

```markdown
# Summary

[Introduction](./title-page.md)
[Glossary](./glossary.md)

# Getting Started
- [Overview](./getting-started/overview.md)
- [Creating](./getting-started/creating.md)
- [Running](./getting-started/running.md)
- [Runtime Options](./getting-started/runtime-options.md)
- [Metrics](./getting-started/metrics.md)

# Configuration
- [Overview](./config/overview.md)
- [Defaults](./config/defaults.md)
- [Rustls](./config/rustls.md)
- [Scheduler](./config/scheduler.md)

# Protocols (NEW SECTION)
- [Overview](./protocols/overview.md)
- [HTTP/3 Support](./protocols/http3.md)
- [GraphQL Support](./protocols/graphql.md)

# Examples
- [Overview](./example/overview.md)
- [Simple](./example/simple.md)
- [Session](./example/session.md)
- [Closure](./example/closure.md)
- [Drupal Memcache](./example/drupal-memcache.md)
- [Umami](./example/umami.md)
- [HTTP/3 Load Test](./example/http3.md) (NEW)
- [GraphQL Load Test](./example/graphql.md) (NEW)
```

**New File: `src/docs/goose-book/src/protocols/overview.md`**

```markdown
# Protocol Support

Goose supports multiple protocols for load testing different types of applications and services. While HTTP/1.1 and HTTP/2 are supported by default, additional protocols can be enabled via feature flags.

## Supported Protocols

### HTTP Family
- **HTTP/1.1**: Default support via reqwest
- **HTTP/2**: Automatic support when server supports it
- **HTTP/3**: Optional support via `http3` feature flag

### Query Languages
- **GraphQL**: Helper methods for GraphQL queries and mutations

### Coming Soon
- **gRPC**: Full support for all RPC patterns (planned)
- **WebSocket**: Persistent connection support (planned)

## Feature Flags

Protocol support is managed through Cargo feature flags to keep builds lightweight:

- `http3`: Enables HTTP/3 support
- `grpc`: Enables gRPC support (future)
- `websocket`: Enables WebSocket support (future)

## Configuration

Protocol-specific configuration options are available through:
- Command-line arguments
- Configuration files
- Programmatic defaults

See individual protocol sections for detailed configuration options.
```

**New File: `src/docs/goose-book/src/protocols/http3.md`**

```markdown
# HTTP/3 Support

HTTP/3 support in Goose is provided as an optional feature that leverages the QUIC protocol for improved performance and connection reliability.

## Enabling HTTP/3

HTTP/3 support requires the `http3` feature to be enabled at compile time:

```toml
[dependencies]
goose = { version = "0.18", features = ["http3"] }
```

Or when running examples:
```bash
cargo run --example http3_loadtest --features http3
```

## Configuration

### Command Line (to disable if needed)
```bash
goose --no-http3
```

### Programmatic Configuration (to disable if needed)
```rust
use goose::prelude::*;

GooseAttack::initialize()?
    .set_default(GooseDefault::NoHttp3, true)?
    // ... rest of configuration
```

### Configuration File (to disable if needed)
```toml
no_http3 = true
```

## How It Works

When the HTTP/3 feature is compiled in, HTTP/3 is preferred by default:

1. Goose will automatically attempt to use HTTP/3 for all requests
2. The server must support HTTP/3 for the protocol to be used  
3. If HTTP/3 is not available, connections fall back to HTTP/1.1 or HTTP/2
4. HTTP/3 can be explicitly disabled with `--no-http3` if needed

## Example Usage

See the [HTTP/3 load test example](../example/http3.md) for a complete working example.

## Performance Considerations

- HTTP/3 can provide better performance over lossy networks
- Initial connection establishment may be slightly slower
- Multiplexing benefits are similar to HTTP/2
- QUIC provides improved congestion control

## Troubleshooting

### Feature Not Available Error
If you see an error about HTTP/3 not being available, ensure the feature is enabled:
```bash
cargo build --features http3
```

### Connection Fallback
If HTTP/3 connections fail, check:
1. Server HTTP/3 support
2. Network firewall settings (UDP port 443)
3. Client network configuration
```

**New File: `src/docs/goose-book/src/protocols/graphql.md`**

```markdown
# GraphQL Support

Goose provides convenient helper methods for load testing GraphQL APIs, making it easy to send queries, mutations, and subscriptions.

## Helper Methods

### `post_graphql`

Send a GraphQL query or mutation:

```rust
use goose::prelude::*;
use serde_json::json;

async fn graphql_query(user: &mut GooseUser) -> TransactionResult {
    let query = r#"
        query GetUsers {
            users {
                id
                name
                email
            }
        }
    "#;
    
    let _goose = user.post_graphql(query, None).await?;
    Ok(())
}
```

### `post_graphql_named`

Send a named GraphQL query for better metrics tracking:

```rust
async fn named_query(user: &mut GooseUser) -> TransactionResult {
    let query = r#"
        query GetUserPosts($userId: ID!, $limit: Int) {
            user(id: $userId) {
                posts(limit: $limit) {
                    id
                    title
                }
            }
        }
    "#;
    
    let variables = json!({
        "userId": "123",
        "limit": 10
    });
    
    let _goose = user.post_graphql_named(query, Some(variables), "GetUserPosts").await?;
    Ok(())
}
```

## Configuration

### GraphQL Endpoint

Configure the GraphQL endpoint (default is `/graphql`):

#### Command Line
```bash
goose --graphql-endpoint /api/graphql
```

#### Programmatic
```rust
GooseAttack::initialize()?
    .set_default(GooseDefault::GraphqlEndpoint, "/api/graphql")?
```

#### Configuration File
```toml
graphql_endpoint = "/api/graphql"
```

## Query Types

### Simple Queries
```rust
let query = r#"{ users { id name } }"#;
user.post_graphql(query, None).await?;
```

### Queries with Variables
```rust
let query = r#"query GetUser($id: ID!) { user(id: $id) { name } }"#;
let variables = json!({"id": "123"});
user.post_graphql(query, Some(variables)).await?;
```

### Mutations
```rust
let mutation = r#"
    mutation CreateUser($input: CreateUserInput!) {
        createUser(input: $input) {
            id
            name
        }
    }
"#;
let variables = json!({"input": {"name": "Test User"}});
user.post_graphql(mutation, Some(variables)).await?;
```

## Error Handling

GraphQL responses are handled like regular HTTP responses:

```rust
let goose = user.post_graphql_named(query, variables, "CreateUser").await?;

// Check HTTP status
if goose.request.status_code != 200 {
    return user.set_failure("HTTP error", &mut goose.request, None, None);
}

// Parse and check GraphQL errors
if let Ok(response) = goose.response {
    let json: serde_json::Value = response.json().await?;
    if json.get("errors").is_some() {
        return user.set_failure("GraphQL error", &mut goose.request, None, None);
    }
}
```

## Example Usage

See the [GraphQL load test example](../example/graphql.md) for a complete working example.

## Best Practices

1. **Use Named Queries**: Always use `post_graphql_named` for better metrics
2. **Variable Substitution**: Use variables instead of string interpolation
3. **Error Checking**: Always check for both HTTP and GraphQL errors
4. **Query Optimization**: Use field selection to minimize response size
5. **Rate Limiting**: Be mindful of GraphQL complexity scoring
```

**New File: `src/docs/goose-book/src/example/http3.md`**

```markdown
# HTTP/3 Load Test Example

This example demonstrates how to use Goose's HTTP/3 support for load testing applications that support the HTTP/3 protocol.

## Requirements

This example requires the `http3` feature to be enabled:

```bash
cargo run --example http3_loadtest --features http3
```

## Configuration

The example shows HTTP/3 enabled by default when the feature is compiled:

```rust
// HTTP/3 is automatically preferred when feature is enabled
GooseAttack::initialize()?
    // No special configuration needed!
    
// To disable HTTP/3 if needed:
// GooseAttack::initialize()?
//     .set_default(GooseDefault::NoHttp3, true)?
```

## Key Features Demonstrated

- HTTP/3 enabled by default when feature is compiled in
- Mixed GET and POST requests over HTTP/3
- JSON payload handling
- Transaction weighting for realistic load patterns
- Optional HTTP/3 disabling if needed

## Server Requirements

To test HTTP/3, you need a server that supports the protocol:

- Nginx with HTTP/3 module
- Cloudflare (supports HTTP/3)
- Chrome/Firefox in development mode
- Custom HTTP/3 test servers

## Running the Example

```bash
# Enable HTTP/3 feature and run
cargo run --example http3_loadtest --features http3

# Run against a specific HTTP/3 enabled server
cargo run --example http3_loadtest --features http3 -- --host https://http3.example.com
```

## Expected Output

The load test will show standard Goose metrics. HTTP/3 usage can be verified through:
- Server logs showing HTTP/3 connections
- Network monitoring tools
- Browser developer tools (if testing web applications)

## Performance Notes

- Initial connection establishment may be slightly slower
- Better performance over lossy networks
- Improved multiplexing compared to HTTP/1.1
- QUIC protocol benefits for mobile networks
```

**New File: `src/docs/goose-book/src/example/graphql.md`**

```markdown
# GraphQL Load Test Example

This example demonstrates comprehensive GraphQL load testing using Goose's GraphQL helper methods.

## Running the Example

```bash
cargo run --example graphql_loadtest
```

## Features Demonstrated

### Query Types
- **Simple Queries**: Basic field selection
- **Parameterized Queries**: Using variables for dynamic queries
- **Complex Nested Queries**: Deep object traversal
- **Mutations**: Creating and modifying data

### Load Testing Patterns
- **Weighted Transactions**: Different frequencies for different operations
- **Realistic Data**: Random IDs and dynamic content
- **Error Handling**: Proper GraphQL error checking
- **Named Requests**: Better metrics tracking

## Configuration Options

```bash
# Custom GraphQL endpoint
cargo run --example graphql_loadtest -- --graphql-endpoint /api/v2/graphql

# Different host
cargo run --example graphql_loadtest -- --host https://api.example.com

# More users and longer duration
cargo run --example graphql_loadtest -- --users 20 --run-time 5m
```

## GraphQL Server Setup

To test with a real GraphQL server, you can use:

### Apollo Server
```javascript
const { ApolloServer } = require('apollo-server');
// ... server setup
```

### GraphQL Playground
Many GraphQL implementations provide a playground interface for testing.

### Mock Servers
- GraphQL mock servers for testing
- Schema-based response generation

## Metrics and Analysis

The example produces metrics for each named GraphQL operation:
- Response times per query type
- Success/failure rates
- Request volume distribution

## Best Practices Shown

1. **Named Operations**: Each GraphQL operation is named for better tracking
2. **Variable Usage**: Proper variable substitution instead of string interpolation
3. **Realistic Load**: Different operation frequencies match real usage
4. **Error Handling**: Checking both HTTP and GraphQL-level errors
5. **Data Variety**: Using random data for more realistic testing
```

### 7. Testing Strategy
### 7. Testing Strategy

#### Unit Tests

Add tests to `src/goose.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // ... existing tests ...

    #[tokio::test]
    async fn test_graphql_helper_methods() {
        let server = MockServer::start();
        let mut user = setup_user(&server).unwrap();

        // Test GraphQL endpoint
        let graphql_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/graphql")
                .header("content-type", "application/json");
            then.status(200)
                .json_body(json!({
                    "data": {
                        "users": [
                            {"id": "1", "name": "Test User"}
                        ]
                    }
                }));
        });

        // Test simple GraphQL query
        let query = "query GetUsers { users { id name } }";
        let response = user.post_graphql(query, None).await.unwrap();
        
        assert!(response.request.success);
        assert_eq!(response.request.status_code, 200);
        graphql_mock.assert_hits(1);

        // Test GraphQL query with variables
        let query_with_vars = "query GetUser($id: ID!) { user(id: $id) { id name } }";
        let variables = json!({"id": "1"});
        
        let response = user.post_graphql_named(query_with_vars, Some(variables), "GetUser").await.unwrap();
        assert_eq!(response.request.name, "GetUser");
        assert!(response.request.success);
    }

    #[cfg(feature = "http3")]
    #[test]
    fn test_http3_configuration() {
        let mut config = GooseConfiguration::default();
        config.prefer_http3 = true;
        
        // Test that HTTP/3 client creation works
        let client = create_reqwest_client(&config);
        assert!(client.is_ok());
    }
}
```

#### Integration Tests

Create `tests/phase1_integration.rs`:

```rust
use goose::prelude::*;
use serde_json::json;

#[tokio::test]
async fn test_graphql_integration() {
    // Test that GraphQL methods integrate properly with the full Goose system
    // This would use a test server that responds to GraphQL queries
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn test_http3_integration() {
    // Test HTTP/3 integration if feature is enabled
}
```

## Detailed Testing Plan

### Unit Tests (`src/goose.rs`)

**New Tests for GraphQL Methods:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;
    
    #[tokio::test]
    async fn test_post_graphql_simple_query() {
        let server = MockServer::start();
        let mut user = setup_user(&server).unwrap();

        let graphql_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/graphql")
                .header("content-type", "application/json")
                .json_body(json!({
                    "query": "query GetUsers { users { id name } }"
                }));
            then.status(200)
                .json_body(json!({
                    "data": {
                        "users": [
                            {"id": "1", "name": "Test User"}
                        ]
                    }
                }));
        });

        let query = "query GetUsers { users { id name } }";
        let response = user.post_graphql(query, None).await.unwrap();
        
        assert!(response.request.success);
        assert_eq!(response.request.status_code, 200);
        assert_eq!(response.request.raw.method, GooseMethod::Post);
        assert_eq!(response.request.raw.url.contains("/graphql"), true);
        graphql_mock.assert_hits(1);
    }

    #[tokio::test]
    async fn test_post_graphql_with_variables() {
        let server = MockServer::start();
        let mut user = setup_user(&server).unwrap();

        let expected_body = json!({
            "query": "query GetUser($id: ID!) { user(id: $id) { id name } }",
            "variables": {"id": "123"}
        });

        let graphql_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/graphql")
                .header("content-type", "application/json")
                .json_body(expected_body);
            then.status(200)
                .json_body(json!({
                    "data": {
                        "user": {"id": "123", "name": "Test User"}
                    }
                }));
        });

        let query = "query GetUser($id: ID!) { user(id: $id) { id name } }";
        let variables = json!({"id": "123"});
        
        let response = user.post_graphql(query, Some(variables)).await.unwrap();
        assert!(response.request.success);
        assert_eq!(response.request.status_code, 200);
        graphql_mock.assert_hits(1);
    }

    #[tokio::test]
    async fn test_post_graphql_named() {
        let server = MockServer::start();
        let mut user = setup_user(&server).unwrap();

        let graphql_mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(200).json_body(json!({"data": {}}));
        });

        let query = "query GetUsers { users { id } }";
        let response = user.post_graphql_named(query, None, "GetUsersTest").await.unwrap();
        
        assert_eq!(response.request.name, "GetUsersTest");
        assert!(response.request.success);
        graphql_mock.assert_hits(1);
    }

    #[tokio::test]
    async fn test_graphql_custom_endpoint() {
        let server = MockServer::start();
        let mut config = GooseConfiguration::default();
        config.graphql_endpoint = "/api/v2/graphql".to_string();
        
        let base_url = get_base_url(Some(server.url("/")), None, None).unwrap();
        let mut user = GooseUser::new(0, "".to_string(), base_url, &config, 0, None).unwrap();

        let graphql_mock = server.mock(|when, then| {
            when.method(POST).path("/api/v2/graphql");
            then.status(200).json_body(json!({"data": {}}));
        });

        let query = "{ users { id } }";
        let response = user.post_graphql(query, None).await.unwrap();
        
        assert!(response.request.success);
        assert!(response.request.raw.url.contains("/api/v2/graphql"));
        graphql_mock.assert_hits(1);
    }

    #[tokio::test]
    async fn test_graphql_error_handling() {
        let server = MockServer::start();
        let mut user = setup_user(&server).unwrap();

        let graphql_mock = server.mock(|when, then| {
            when.method(POST).path("/graphql");
            then.status(400)
                .json_body(json!({
                    "errors": [
                        {"message": "Syntax error"}
                    ]
                }));
        });

        let query = "invalid graphql syntax {";
        let response = user.post_graphql(query, None).await.unwrap();
        
        assert!(!response.request.success);
        assert_eq!(response.request.status_code, 400);
        graphql_mock.assert_hits(1);
    }

    #[cfg(feature = "http3")]
    #[test]
    fn test_http3_client_configuration() {
        let config = GooseConfiguration::default(); // HTTP/3 enabled by default
        
        let client = create_reqwest_client(&config);
        assert!(client.is_ok());
        
        // Test disabling HTTP/3
        let mut config_disabled = GooseConfiguration::default();
        config_disabled.no_http3 = true;
        let client_disabled = create_reqwest_client(&config_disabled);
        assert!(client_disabled.is_ok());
    }

    #[cfg(not(feature = "http3"))]
    #[test]
    fn test_http3_feature_disabled() {
        // Test that HTTP/3 related code is not compiled without feature
        let config = GooseConfiguration::default();
        
        // This should compile fine even without HTTP/3 feature
        let client = create_reqwest_client(&config);
        assert!(client.is_ok());
    }
}
```

### Configuration Tests (`src/config.rs`)

**New Tests for Configuration Options:**

```rust
#[cfg(test)]
mod test {
    use super::*;
    
    #[test]
    fn test_graphql_endpoint_default() {
        let config = GooseConfiguration::default();
        assert_eq!(config.graphql_endpoint, "");
    }

    #[test]
    fn test_graphql_endpoint_configuration() {
        let mut config = GooseConfiguration::default();
        config.graphql_endpoint = "/api/graphql".to_string();
        
        let defaults = GooseDefaults::default();
        config.configure(&defaults);
        
        assert_eq!(config.graphql_endpoint, "/api/graphql");
    }

    #[cfg(feature = "http3")]
    #[test]
    fn test_http3_disabled_by_default() {
        let config = GooseConfiguration::default();
        assert_eq!(config.no_http3, false); // HTTP/3 enabled by default
    }

    #[cfg(feature = "http3")]
    #[test]
    fn test_http3_can_be_disabled() {
        let mut config = GooseConfiguration::default();
        config.no_http3 = true;
        
        let defaults = GooseDefaults::default();
        config.configure(&defaults);
        
        assert_eq!(config.no_http3, true);
    }

    #[test]
    fn test_set_graphql_endpoint_default() {
        let goose_attack = GooseAttack::initialize()
            .unwrap()
            .set_default(GooseDefault::GraphqlEndpoint, "/custom/graphql")
            .unwrap();
        
        assert_eq!(goose_attack.defaults.graphql_endpoint, Some("/custom/graphql".to_string()));
    }

    #[cfg(feature = "http3")]
    #[test]
    fn test_set_http3_disable_default() {
        let goose_attack = GooseAttack::initialize()
            .unwrap()
            .set_default(GooseDefault::NoHttp3, true)
            .unwrap();
        
        assert_eq!(goose_attack.defaults.no_http3, Some(true));
    }
}
```

### Integration Tests

**New File: `tests/phase1_protocols.rs`**

```rust
use goose::prelude::*;
use httpmock::MockServer;
use serde_json::json;

#[tokio::test]
async fn test_graphql_integration() {
    let server = MockServer::start();
    
    // Set up GraphQL mock endpoint
    let _graphql_mock = server.mock(|when, then| {
        when.method(httpmock::Method::POST)
            .path("/graphql")
            .header("content-type", "application/json");
        then.status(200)
            .json_body(json!({
                "data": {
                    "users": [{"id": "1", "name": "Test"}]
                }
            }));
    });

    async fn graphql_transaction(user: &mut GooseUser) -> TransactionResult {
        let query = r#"query GetUsers { users { id name } }"#;
        let _goose = user.post_graphql_named(query, None, "GetUsers").await?;
        Ok(())
    }

    let _goose_metrics = GooseAttack::initialize()?
        .register_scenario(
            scenario!("GraphQLTest")
                .set_host(&server.url("/"))
                .register_transaction(transaction!(graphql_transaction))
        )
        .set_default(GooseDefault::RunTime, 1)?
        .set_default(GooseDefault::Users, 1)?
        .set_default(GooseDefault::NoMetrics, true)?
        .execute()
        .await?;
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn test_http3_integration() {
    // This test requires an HTTP/3 capable server
    // For now, just test that configuration works
    
    async fn http3_transaction(user: &mut GooseUser) -> TransactionResult {
        let _goose = user.get("/").await?;
        Ok(())
    }

    let server = MockServer::start();

    let _goose_metrics = GooseAttack::initialize()?
        .register_scenario(
            scenario!("HTTP3Test")
                .set_host(&server.url("/"))
                .register_transaction(transaction!(http3_transaction))
        )
        // HTTP/3 enabled by default when feature compiled in
        // .set_default(GooseDefault::NoHttp3, true)? // Uncomment to disable
        .set_default(GooseDefault::RunTime, 1)?
        .set_default(GooseDefault::Users, 1)?
        .set_default(GooseDefault::NoMetrics, true)?
        .execute()
        .await?;
}

#[tokio::test]
async fn test_mixed_protocol_load_test() {
    let server = MockServer::start();
    
    // Mock both regular HTTP and GraphQL endpoints
    let _http_mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/api/data");
        then.status(200).body("OK");
    });
    
    let _graphql_mock = server.mock(|when, then| {
        when.method(httpmock::Method::POST).path("/graphql");
        then.status(200).json_body(json!({"data": {"result": "success"}}));
    });

    async fn http_transaction(user: &mut GooseUser) -> TransactionResult {
        let _goose = user.get("/api/data").await?;
        Ok(())
    }

    async fn graphql_transaction(user: &mut GooseUser) -> TransactionResult {
        let query = r#"{ result }"#;
        let _goose = user.post_graphql(query, None).await?;
        Ok(())
    }

    let _goose_metrics = GooseAttack::initialize()?
        .register_scenario(
            scenario!("MixedProtocolTest")
                .set_host(&server.url("/"))
                .register_transaction(transaction!(http_transaction))
                .register_transaction(transaction!(graphql_transaction))
        )
        .set_default(GooseDefault::RunTime, 2)?
        .set_default(GooseDefault::Users, 2)?
        .set_default(GooseDefault::NoMetrics, true)?
        .execute()
        .await?;
}
```

### Example Tests

**New File: `tests/example_tests.rs`**

```rust
use std::process::Command;

#[test]
fn test_graphql_example_compiles() {
    let output = Command::new("cargo")
        .args(&["check", "--example", "graphql_loadtest"])
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(), 
            "GraphQL example failed to compile: {}", 
            String::from_utf8_lossy(&output.stderr));
}

#[test]
#[cfg(feature = "http3")]
fn test_http3_example_compiles() {
    let output = Command::new("cargo")
        .args(&["check", "--example", "http3_loadtest", "--features", "http3"])
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(), 
            "HTTP/3 example failed to compile: {}", 
            String::from_utf8_lossy(&output.stderr));
}

#[test]
#[cfg(not(feature = "http3"))]
fn test_http3_example_fails_without_feature() {
    let output = Command::new("cargo")
        .args(&["check", "--example", "http3_loadtest"])
        .output()
        .expect("Failed to run cargo check");

    assert!(!output.status.success(), 
            "HTTP/3 example should fail without feature flag");
}
```

## Implementation Order

### Step 1: Core Configuration (Day 1)
1. Update `Cargo.toml` with feature flags
2. Modify `src/config.rs` with new configuration options
3. Add configuration tests
4. Update configuration parsing and validation logic

### Step 2: HTTP Client Enhancement (Day 1-2)
1. Update `create_reqwest_client()` function
2. Add conditional compilation for HTTP/3
3. Add HTTP/3 configuration tests
4. Test client creation with and without HTTP/3

### Step 3: GraphQL Helper Methods (Day 2-3)
1. Add `post_graphql()` method to `GooseUser`
2. Add `post_graphql_named()` method
3. Write comprehensive unit tests for GraphQL methods
4. Add GraphQL integration tests

### Step 4: Example Applications (Day 3-4)
1. Create `examples/http3_loadtest.rs`
2. Create `examples/graphql_loadtest.rs`
3. Add example compilation tests
4. Test examples with different configurations

### Step 5: Documentation and Testing (Day 4-5)
1. Create Goose Book documentation sections
2. Write example documentation pages
3. Add comprehensive integration tests
4. Validate backward compatibility
5. Performance testing and benchmarks

### Step 6: Final Validation (Day 5)
1. Run full test suite
2. Test feature flag combinations
3. Validate documentation accuracy
4. Performance regression testing

## Validation Checklist

### Functionality
- [ ] HTTP/3 feature compiles correctly when enabled
- [ ] HTTP/3 feature is absent when disabled (no runtime overhead)
- [ ] GraphQL methods work with various query types (simple, with variables, mutations)
- [ ] Configuration options work via CLI and programmatic APIs
- [ ] Examples run successfully with correct feature flags
- [ ] All existing functionality continues to work
- [ ] New functions have complete rustdoc documentation
- [ ] All new code paths covered by tests

### Performance  
- [ ] No performance regression for default builds
- [ ] HTTP/3 performance is comparable to HTTP/1.1/2 when enabled
- [ ] GraphQL requests show appropriate metrics
- [ ] Memory usage remains stable
- [ ] Benchmarks confirm no overhead for unused features

### Documentation
- [ ] All new functions have rustdoc comments with examples
- [ ] Goose Book sections are complete and accurate
- [ ] Example documentation explains requirements and usage
- [ ] Configuration options documented in multiple places
- [ ] Migration notes for users adopting new features

### Testing
- [ ] Unit tests cover all new functions with edge cases
- [ ] Integration tests validate end-to-end functionality
- [ ] Example compilation tests prevent regressions
- [ ] Feature flag combinations tested
- [ ] Error conditions properly tested

### Ergonomics
- [ ] Clear error messages when HTTP/3 requested but not compiled
- [ ] Intuitive API for GraphQL queries matching existing patterns
- [ ] Good documentation and examples with clear requirements
- [ ] Standard Rust patterns followed (feature flags, conditional compilation)
- [ ] Helpful CLI help text for new options

## Success Metrics

1. **Zero Breaking Changes**: All existing code continues to work without modification
2. **Optional HTTP/3**: Feature only included when explicitly requested, zero runtime overhead otherwise
3. **Ergonomic GraphQL**: Simple, intuitive API for GraphQL queries that matches existing HTTP patterns
4. **Performance Maintained**: No regression in baseline performance, benchmarks confirm
5. **Comprehensive Documentation**: Complete rustdoc, Goose Book sections, and examples
6. **Complete Test Coverage**: All new functionality covered by unit and integration tests
7. **Clear Error Messages**: Helpful guidance when features aren't available or misconfigured

## Next Steps After Phase 1

Once Phase 1 is complete, we'll have:
- Proven the multi-protocol architecture concept
- Established patterns for optional protocol support
- Created a foundation for gRPC and WebSocket implementation
- Validated the approach with HTTP/3 and GraphQL

This sets us up perfectly for Phase 2 (gRPC implementation) and Phase 3 (WebSocket support).
