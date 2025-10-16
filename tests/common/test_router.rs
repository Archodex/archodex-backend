// Test router helpers for integration testing
#![allow(dead_code)]

use super::providers::TestResourcesDbFactory;
use archodex_backend::test_support::{
    AppState, AuthProvider, DBConnection, create_router_with_state,
};
use axum::{Router, routing::get};
use std::sync::Arc;

/// Creates a simple test router without authentication middleware
///
/// This router is useful for testing endpoints that don't require authentication,
/// or for testing handler logic in isolation from auth concerns.
///
/// # Examples
///
/// ```ignore
/// use tower::ServiceExt; //  for oneshot()
///
/// #[tokio::test]
/// async fn test_health_endpoint() {
///     let app = create_test_router();
///
///     let response = app
///         .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
///         .await
///         .unwrap();
///
///     assert_eq!(response.status(), StatusCode::OK);
/// }
/// ```
pub fn create_test_router() -> Router {
    Router::new().route("/health", get(|| async { "Ok" }))
}

/// Creates a test router with injected database connections and authentication provider
///
/// This function creates a full application router with dependency-injected test databases
/// and authentication. The router includes all routes and middleware from the production
/// application, but uses in-memory test databases and `FixedAuthProvider` for authentication.
///
/// # Parameters
/// * `accounts_db` - In-memory database for accounts (authentication and authorization)
/// * `resources_db` - In-memory database for resources (per-account data)
/// * `auth_provider` - Authentication provider (typically `FixedAuthProvider` for tests)
///
/// # Examples
///
/// ```ignore
/// use tower::ServiceExt;
/// use axum::http::{Request, StatusCode};
/// use axum::body::Body;
///
/// #[tokio::test]
/// async fn test_report_endpoint() {
///     // Create test databases
///     let accounts_db = create_test_accounts_db().await;
///     let resources_db = create_test_resources_db().await;
///
///     // Seed test account
///     seed_test_account(&accounts_db, "test_acc_123").await;
///
///     // Create auth provider
///     let auth_provider = create_fixed_auth_provider("test_acc_123", 99999);
///
///     // Create router with injected dependencies
///     let app = create_test_router_with_state(
///         accounts_db.clone(),
///         resources_db.clone(),
///         auth_provider
///     );
///
///     // Make request (no Authorization header needed - FixedAuthProvider handles it)
///     let response = app
///         .oneshot(Request::builder()
///             .uri("/report")
///             .method("POST")
///             .body(Body::from("{}"))
///             .unwrap())
///         .await
///         .unwrap();
///
///     assert_eq!(response.status(), StatusCode::OK);
///
///     // Validate database state
///     // Query resources_db to verify data was persisted
/// }
/// ```
pub fn create_test_router_with_state(
    accounts_db: DBConnection,
    resources_db: DBConnection,
    auth_provider: Arc<dyn AuthProvider>,
) -> Router {
    let factory = TestResourcesDbFactory {
        accounts_db,
        resources_db,
    };

    let state = AppState {
        resources_db_factory: Arc::new(factory),
        auth_provider,
    };

    create_router_with_state(state)
}
