// Test database factory implementations for dependency injection
//
// This module provides test-specific implementations of the ResourcesDbFactory trait,
// allowing tests to inject in-memory database connections into the application state.
#![allow(dead_code)]

use archodex_backend::test_support::{DBConnection, ResourcesDbFactory};
use archodex_error::anyhow::Result;
use async_trait::async_trait;

/// Test implementation of `ResourcesDbFactory` that returns pre-configured in-memory databases
///
/// This factory is used in tests to inject in-memory `SurrealDB` connections into the
/// application state. Unlike the production `GlobalResourcesDbFactory` which uses global
/// connection pools, this factory returns clones of pre-configured test databases.
///
/// # Design
/// - Holds references to both accounts and resources test databases
/// - Returns clones of these databases regardless of `account_id` o`service_url`rl parameters
/// - Enables complete test isolation by using separate database instances per test
///
/// # Usage
///
/// ```ignore
/// use archodex_backend::state::AppState;
/// use std::sync::Arc;
///
/// #[tokio::test]
/// async fn test_with_injected_databases() {
///     let accounts_db = create_test_accounts_db().await;
///     let resources_db = create_test_resources_db().await;
///
///     let factory = TestResourcesDbFactory {
///         accounts_db: accounts_db.clone(),
///         resources_db: resources_db.clone(),
///     };
///
///     let state = AppState {
///         resources_db_factory: Arc::new(factory),
///     };
///
///     let router = create_router_with_state(state);
///     // Make requests to router, which will use injected databases
/// }
/// ```
pub struct TestResourcesDbFactory {
    /// Pre-configured accounts database connection
    pub accounts_db: DBConnection,
    /// Pre-configured resources database connection
    pub resources_db: DBConnection,
}

#[async_trait]
impl ResourcesDbFactory for TestResourcesDbFactory {
    /// Returns the test accounts database connection
    ///
    /// Ignores all parameters and always returns a clone of the test accounts database.
    /// This allows tests to seed and query the same database instance.
    async fn create_accounts_connection(&self) -> Result<DBConnection> {
        Ok(self.accounts_db.clone())
    }

    /// Returns the test resources database connection
    ///
    /// Ignores `account_id` and `service_url` parameters and always returns a clone of
    /// the test resources database. This allows tests to seed and query the same
    /// database instance regardless of which account is being accessed.
    ///
    /// # Parameters
    /// - `account_id`: Ignored in test implementation
    /// - `service_url`: Ignored in test implementation
    async fn create_resources_connection(
        &self,
        _account_id: &str,
        _service_url: Option<&str>,
    ) -> Result<DBConnection> {
        Ok(self.resources_db.clone())
    }
}
