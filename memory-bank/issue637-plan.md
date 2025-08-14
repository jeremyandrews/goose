# Issue #637: Eliminate String Formatting in Metrics Aggregation Hot Path

## Problem Statement
The `record_request_metric()` function in `src/metrics.rs` performs string formatting for every HTTP request:

```rust
let key = format!("{} {}", request_metric.raw.method, request_metric.name);
```

This creates heap allocations and CPU overhead for every single request made during load testing. With high-throughput tests making millions of requests, this becomes a significant performance bottleneck.

## Solution: Pre-computed Cow Keys
Use `Cow<'static, str>` to eliminate allocations for common request patterns while gracefully handling dynamic cases.

### Why Cow<'static, str>?
- **Zero allocation** for common static patterns (GET /, POST /login, etc.)
- **Graceful fallback** to owned strings for dynamic requests
- **Idiomatic Rust** for "mostly static, sometimes owned" data
- **Minimal code changes** compared to integer hash approach

## Implementation Plan

### Step 1: Add formatted_key field to GooseRequestMetric
**File:** `src/metrics.rs`

```rust
pub struct GooseRequestMetric {
    // ... existing fields
    pub formatted_key: Cow<'static, str>,  // NEW: Pre-computed key
}
```

### Step 2: Create key computation function
**File:** `src/metrics.rs`

```rust
fn compute_request_key(method: &GooseMethod, name: &str) -> Cow<'static, str> {
    match (method, name) {
        // Zero-allocation cases for common patterns
        (GooseMethod::Get, "/") => Cow::Borrowed("GET /"),
        (GooseMethod::Get, "/login") => Cow::Borrowed("GET /login"),
        (GooseMethod::Post, "/login") => Cow::Borrowed("POST /login"),
        (GooseMethod::Get, "/logout") => Cow::Borrowed("GET /logout"),
        (GooseMethod::Post, "/logout") => Cow::Borrowed("POST /logout"),
        (GooseMethod::Get, "/api/users") => Cow::Borrowed("GET /api/users"),
        (GooseMethod::Post, "/api/users") => Cow::Borrowed("POST /api/users"),
        // Add more common patterns as needed...
        
        // Dynamic fallback - allocate only when needed
        _ => Cow::Owned(format!("{} {}", method, name)),
    }
}
```

### Step 3: Update GooseRequestMetric::new()
**File:** `src/metrics.rs`

```rust
impl GooseRequestMetric {
    pub(crate) fn new(
        raw: GooseRawRequest,
        transaction_detail: TransactionDetail,
        name: &str,
        elapsed: u128,
        user: usize,
    ) -> Self {
        let formatted_key = compute_request_key(&raw.method, name);
        
        GooseRequestMetric {
            elapsed: elapsed as u64,
            scenario_index: transaction_detail.scenario_index,
            scenario_name: transaction_detail.scenario_name.to_string(),
            transaction_index: transaction_detail.transaction_index.to_string(),
            transaction_name: transaction_detail.transaction_name,
            raw,
            name: name.to_string(),
            formatted_key,  // NEW: Store pre-computed key
            final_url: "".to_string(),
            redirected: false,
            response_time: 0,
            status_code: 0,
            success: true,
            update: false,
            user,
            error: "".to_string(),
            coordinated_omission_elapsed: 0,
            user_cadence: 0,
        }
    }
}
```

### Step 4: Update hot path to use pre-computed key
**File:** `src/metrics.rs`

```rust
async fn record_request_metric(&mut self, request_metric: &GooseRequestMetric) {
    // Use pre-computed key - NO MORE ALLOCATION!
    let key = request_metric.formatted_key.as_ref();
    let mut merge_request = match self.metrics.requests.get(key) {
        Some(m) => m.clone(),
        None => GooseRequestMetricAggregate::new(
            &request_metric.name,
            request_metric.raw.method.clone(),
            0,
        ),
    };
    
    // ... rest of function unchanged
    
    self.metrics.requests.insert(key.to_string(), merge_request);
}
```

### Step 5: Update tests and validation
- Ensure all existing tests pass
- Add specific tests for key generation
- Verify performance improvement with benchmarks

## Expected Performance Impact
- **Eliminates string formatting** in the hot path for every request
- **Zero heap allocations** for common request patterns (GET /, POST /login, etc.)
- **Minimal allocations** only for truly dynamic requests
- **Faster HashMap operations** due to reduced allocation pressure
- **Lower memory usage** and reduced garbage collection pressure

## Files to Modify
- `src/metrics.rs` - Main implementation
- Add tests to verify key generation works correctly
- Update any code that relies on the exact format (unlikely)

## Risk Assessment
- **Low risk**: Internal implementation detail, no API changes
- **Backward compatible**: All existing functionality preserved  
- **Easily testable**: Can verify keys match expected patterns
- **Incremental**: Can be implemented and tested step by step

## Verification Strategy
1. Run existing test suite to ensure no regressions
2. Add unit tests for `compute_request_key()` function
3. Performance test with high request volume to measure improvement
4. Memory profiling to confirm allocation reduction

This approach provides significant performance benefits with minimal code changes and maintains all existing functionality.
