// Shared test helpers for Archodex backend testing
//
// This module provides helper functions for:
// - Database setup (in-memory SurrealDB instances)
// - Test data generation (fixtures for accounts, resources, events, reports)
// - Test routers (HTTP testing with auth bypass)
// - Database factory providers (dependency injection for tests)

pub mod auth;
pub mod db;
pub mod fixtures;
pub mod providers;
pub mod test_router;

// Re-export commonly used test helpers for convenience
// Note: Different test binaries use different subsets, so some may be unused in each binary
#[allow(unused_imports)]
pub use auth::create_fixed_auth_provider;
#[allow(unused_imports)]
pub use db::{
    create_test_accounts_db, create_test_resources_db, seed_test_account, seed_test_api_key,
};
#[allow(unused_imports)]
pub use fixtures::{create_simple_test_report_request, create_test_report_request};
#[allow(unused_imports)]
pub use test_router::{create_test_router, create_test_router_with_state};
