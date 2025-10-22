# Testing in Archodex Backend

This directory contains integration and unit tests for the Archodex backend. The testing framework uses dependency
injection to replace production database connections and authentication with in-memory implementations, making tests
fast and isolated.

## Running Tests

```bash
# Run all tests
cargo test -p archodex-backend

# Run only unit tests (inline in src/)
cargo test -p archodex-backend --lib

# Run only integration tests (in tests/)
cargo test -p archodex-backend --test '*'

# Run specific test
cargo test -p archodex-backend test_middleware_loads_account

# Show println output
cargo test -p archodex-backend -- --nocapture

# Show backtraces on failure
RUST_BACKTRACE=1 cargo test -p archodex-backend
```

## Writing Tests

### Unit Tests

Unit tests live inline with your code using `#[cfg(test)]`:

```rust
// In src/your_module.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_logic() {
        let result = your_function(input);
        assert_eq!(result, expected);
    }
}
```

Use unit tests for pure functions, type conversions, and validation logic that doesn't need external dependencies.

Examples: see `test_principal_chain_id_part_round_trip` in `src/principal_chain.rs`

### Integration Tests

Integration tests live in the `tests/` directory and test complete request flows through the HTTP layer, middleware,
handlers, and database.

Here's a complete example:

```rust
// tests/your_feature_test.rs
mod common;

use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;

#[tokio::test]
async fn test_report_endpoint() {
    // Create in-memory databases
    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Seed test data
    let account_id = "test_account_123";
    let key_id = 99999;
    common::seed_test_account(&accounts_db, account_id).await;
    common::seed_test_api_key(&resources_db, key_id).await;

    // Create auth provider that bypasses token validation
    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    // Create router with injected dependencies
    let app = common::create_test_router_with_state(
        accounts_db,
        resources_db,
        auth_provider,
    );

    let payload = common::create_simple_test_report_request();

    // Make request (no Authorization header needed with FixedAuthProvider)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/report")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&payload).unwrap()))
                .unwrap()
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

See `tests/report_with_auth_test.rs` for more examples.

### Test Helpers

The `tests/common/` directory provides helpers for common test operations:

```rust
// Database creation
let accounts_db = common::create_test_accounts_db().await;
let resources_db = common::create_test_resources_db().await;

// Seeding test data
common::seed_test_account(&accounts_db, "acc_123").await;
common::seed_test_api_key(&resources_db, 99999).await;

// Authentication bypass
let auth = common::create_fixed_auth_provider("acc_123", 99999);

// Router creation
let app = common::create_test_router_with_state(accounts_db, resources_db, auth);

// Test data generation
let payload = common::create_simple_test_report_request();
```

## How It Works

### Dependency Injection

The testing framework uses trait-based dependency injection through `AppState`:

```rust
pub struct AppState {
    pub resources_db_factory: Arc<dyn ResourcesDbFactory + Send + Sync>,
    pub auth_provider: Arc<dyn AuthProvider>,
}
```

This state is passed to all Axum handlers, allowing us to inject different implementations for production vs testing.

### Database Factory

The `ResourcesDbFactory` trait abstracts database connection creation:

- **Production**: `GlobalResourcesDbFactory` uses global connection pools
- **Tests**: `TestResourcesDbFactory` returns pre-configured in-memory databases

This means tests can seed data into a database, pass it to the router, and verify the results afterward—all using the
same in-memory instance.

### Authentication Provider

The `AuthProvider` trait abstracts authentication:

- **Production**: `RealAuthProvider` validates JWT/API keys cryptographically
- **Tests**: `FixedAuthProvider` returns pre-configured auth context without validation

This lets tests focus on handler logic without dealing with token generation or cryptographic operations.

### In-Memory Databases

Tests use SurrealDB's `kv-mem` backend, which:

- Starts in microseconds (no container startup delay)
- Runs entirely in memory (no cleanup needed)
- Provides identical API to production backends (RocksDB, DynamoDB)
- Isolates tests completely (each test gets fresh databases)

The `kv-mem` feature is only enabled in `dev-dependencies`, so it never ships to production.

### Feature Flags

The `test-support` feature gates test utilities so they don't compile into production binaries. It's automatically
enabled for integration tests:

```toml
[dev-dependencies]
archodex-backend = { path = ".", features = ["test-support"] }
```

This exposes types like `AppState`, `DBConnection`, and test implementations through the
`archodex_backend::test_support` module.

## Common Patterns

### Testing Public Endpoints

For endpoints without authentication (like `/health`):

```rust
#[tokio::test]
async fn test_health_endpoint() {
    let app = common::create_test_router();

    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

### Testing Authenticated Endpoints

For endpoints that require authentication:

```rust
#[tokio::test]
async fn test_authenticated_endpoint() {
    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Seed required data
    common::seed_test_account(&accounts_db, "acc_123").await;
    common::seed_test_api_key(&resources_db, 99999).await;

    // Bypass authentication
    let auth = common::create_fixed_auth_provider("acc_123", 99999);

    let app = common::create_test_router_with_state(accounts_db, resources_db, auth);

    // Make request (no Authorization header needed)
    let response = app.oneshot(/* ... */).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

### Testing Error Cases

Test that middleware properly handles missing accounts or invalid keys:

```rust
#[tokio::test]
async fn test_rejects_nonexistent_account() {
    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Don't seed account - test authentication with missing account
    let auth = common::create_fixed_auth_provider("nonexistent", 99999);
    let app = common::create_test_router_with_state(accounts_db, resources_db, auth);

    let response = app.oneshot(/* ... */).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

### Verifying Database Operations

Access the injected database to verify operations:

```rust
#[tokio::test]
async fn test_creates_resources() {
    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // ... setup and make request ...

    // Verify data was written
    if let archodex_backend::test_support::DBConnection::Concurrent(ref db) = resources_db {
        use surrealdb::sql::Thing;

        let resources: Vec<Thing> = db
            .query("SELECT VALUE id FROM resource")
            .await
            .unwrap()
            .take(0)
            .unwrap();

        assert!(!resources.is_empty());
    }
}
```

## Test Quality

Write tests that validate real business logic:

**Good tests:**

- Test actual production code behavior
- Validate business rules and error handling
- Test integration flows (HTTP → middleware → handler → database)
- Verify authentication and authorization logic

**Bad tests:**

- Testing "1 + 1 = 2" just to have a passing test
- Testing mock functions that return hardcoded values
- Testing trivial getters/setters with no logic

If you find yourself writing a meaningless test, stop and reconsider what you're trying to validate.

## Troubleshooting

### "Cannot find module `common`"

Ensure `tests/common/mod.rs` exists (NOT `tests/common.rs`). Rust treats `tests/common/` as a module only if it contains
`mod.rs`.

### "kv-mem feature not enabled"

Check that `Cargo.toml` has the `kv-mem` feature in dev-dependencies:

```toml
[dev-dependencies]
surrealdb = { version = "= 2.3.7", features = ["rustls", "kv-mem"] }
```

### Tests timing out

- Ensure `#[tokio::test]` is used for async tests (not `#[test]`)
- Check for infinite loops or blocking operations
- Verify database migrations complete successfully

### "Type X is private"

The `test-support` feature gates certain types. Ensure your test file is in the `tests/` directory or uses
`#[cfg(test)]`, which automatically enables the feature.

### Authentication errors in tests

Tests using `FixedAuthProvider` don't need Authorization headers. If you're getting auth errors:

- Verify you're using `create_test_router_with_state()` not `create_test_router()`
- Check that you've seeded both the test account and API key
- Ensure the account_id and key_id in FixedAuthProvider match the seeded data

## File Reference

### Production Code

- `src/state.rs` - AppState and ResourcesDbFactory trait
- `src/auth/provider.rs` - AuthProvider trait and implementations
- `src/db.rs` - Production database connection management
- `src/router.rs` - Router creation with dependency injection
- `src/lib.rs` - test_support module exports

### Test Infrastructure

- `tests/common/mod.rs` - Test helper re-exports
- `tests/common/db.rs` - Database creation and seeding
- `tests/common/providers.rs` - TestResourcesDbFactory implementation
- `tests/common/auth.rs` - FixedAuthProvider helpers
- `tests/common/fixtures.rs` - Test data generation
- `tests/common/test_router.rs` - Router creation with injected state

### Example Tests

- `tests/health_check_integration_test.rs` - Simple endpoint test
- `tests/report_with_auth_test.rs` - Complete DI integration tests
- `src/principal_chain.rs` - Unit test examples
