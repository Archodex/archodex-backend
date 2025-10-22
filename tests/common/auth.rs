// Test authentication helpers
#![allow(dead_code)]

use archodex_backend::test_support::{AuthProvider, FixedAuthProvider};
use std::sync::Arc;

/// Creates a `FixedAuthProvider` for testing
///
/// This creates an authentication provider that bypasses real validation
/// and returns pre-configured credentials. Useful for integration tests
/// that need to simulate authenticated requests without real JWT tokens.
///
/// # Parameters
/// - `account_id`: The account ID to return for all authentication requests
/// - `key_id`: The key ID to return for all authentication requests
///
/// # Example
/// ```ignore
/// let auth_provider = create_fixed_auth_provider("test_account_123", 99999);
/// let state = AppState {
///     resources_db_factory: Arc::new(test_factory),
///     auth_provider,
/// };
/// let router = create_router_with_state(state);
/// // Now all requests to this router will be authenticated as test_account_123
/// ```
pub fn create_fixed_auth_provider(account_id: &str, key_id: u32) -> Arc<dyn AuthProvider> {
    Arc::new(FixedAuthProvider::new(account_id, key_id))
}
