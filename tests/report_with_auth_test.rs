// Integration tests for /report endpoint with dependency-injected authentication
//
// These tests validate the complete request flow with trait-based dependency injection:
// - AuthProvider trait (FixedAuthProvider for tests, RealAuthProvider for production)
// - report_api_key_account middleware (account loading, key validation)
// - report::report handler (business logic)
//
// Architecture: All tests use dependency injection with FixedAuthProvider to control
// authentication behavior. This enables testing middleware logic without coupling to
// JWT parsing or token format details.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

// Phase 5: User Story 3 - Middleware tests with injected databases
// These tests validate that middleware correctly uses dependency injection

#[tokio::test]
async fn test_middleware_rejects_nonexistent_account() {
    // Test that middleware handles the case when account doesn't exist in database
    // This validates the account loading logic

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // FixedAuthProvider will authenticate with this account_id, but we don't seed it
    let nonexistent_account_id = "account_does_not_exist";
    let key_id = 77777;
    let auth_provider = common::create_fixed_auth_provider(nonexistent_account_id, key_id);

    // Create router with injected databases (account NOT seeded)
    let app = common::create_test_router_with_state(accounts_db, resources_db, auth_provider);

    let report_payload = common::create_simple_test_report_request();

    // POST to /report - FixedAuthProvider authenticates, but account load fails
    let response = app
        .oneshot(
            Request::builder()
                .uri("/report")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&report_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Middleware should return 404 when account doesn't exist in database"
    );
}

#[tokio::test]
async fn test_middleware_invalid_api_key_rejected_with_injected_db() {
    // T019: Test that authentication middleware rejects when API key doesn't exist
    // This verifies middleware validates API keys in the injected resources database

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Seed account but NOT the API key - this tests API key validation
    let account_id = "test_acc_no_key";
    let key_id = 88888;
    let _account = common::seed_test_account(&accounts_db, account_id).await;
    // NOTE: NOT calling seed_test_api_key() - key doesn't exist

    // Create FixedAuthProvider that will authenticate successfully
    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    // Create router with injected databases
    let app = common::create_test_router_with_state(accounts_db, resources_db, auth_provider);

    let report_payload = common::create_test_report_request();

    // POST to /report - FixedAuthProvider authenticates, account exists, but API key validation fails
    let response = app
        .oneshot(
            Request::builder()
                .uri("/report")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&report_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Middleware should reject when API key doesn't exist in injected database"
    );
}

#[tokio::test]
async fn test_middleware_loads_account_from_injected_database() {
    // T020: Test that valid auth token loads account from injected database
    // This verifies middleware correctly accesses the injected accounts_db

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Seed test account in the injected accounts database
    let account_id = "test_acc_456";
    let key_id = 99999;
    let _account = common::seed_test_account(&accounts_db, account_id).await;

    // Seed test API key in resources database (middleware validates key exists)
    common::seed_test_api_key(&resources_db, key_id).await;

    // Create FixedAuthProvider that bypasses authentication
    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    // Create router with injected databases and auth provider
    let app =
        common::create_test_router_with_state(accounts_db, resources_db.clone(), auth_provider);

    let report_payload = common::create_simple_test_report_request();

    // POST to /report - no Authorization header needed with FixedAuthProvider
    let response = app
        .oneshot(
            Request::builder()
                .uri("/report")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&report_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Handler should receive Extension<AuthedAccount> with correct account data
    // We can verify by checking the response includes account-specific behavior
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Middleware should load account from injected database and allow request"
    );

    // Verify resources were created in the injected resources_db
    if let archodex_backend::test_support::DBConnection::Concurrent(ref db) = resources_db {
        use surrealdb::sql::Thing;

        // Query resources created by the report
        let resources: Vec<Thing> = db
            .query("SELECT VALUE id FROM resource")
            .await
            .unwrap()
            .take(0)
            .unwrap();

        assert!(
            !resources.is_empty(),
            "Resources should be created in injected database"
        );
    }
}

#[tokio::test]
async fn test_middleware_uses_resources_db_factory() {
    // T021: Test that middleware uses TestResourcesDbFactory to get resources DB
    // This verifies the factory pattern works correctly with injected databases

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Seed test account
    let account_id = "test_acc_789";
    let key_id = 99999;
    let _account = common::seed_test_account(&accounts_db, account_id).await;

    // Seed test API key in resources database (middleware validates key exists)
    common::seed_test_api_key(&resources_db, key_id).await;

    // Create FixedAuthProvider that bypasses authentication
    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    // Create router - the TestResourcesDbFactory will be used by middleware
    let app =
        common::create_test_router_with_state(accounts_db, resources_db.clone(), auth_provider);

    let report_payload = common::create_simple_test_report_request();

    // Make request - middleware should use factory to get resources DB
    // No Authorization header needed with FixedAuthProvider
    let response = app
        .oneshot(
            Request::builder()
                .uri("/report")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&report_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Handler should receive AuthedAccount with resources_db from factory"
    );

    // Verify the factory provided the correct database by checking data persisted
    if let archodex_backend::test_support::DBConnection::Concurrent(ref db) = resources_db {
        use surrealdb::sql::Thing;

        let resources: Vec<Thing> = db
            .query("SELECT VALUE id FROM resource")
            .await
            .unwrap()
            .take(0)
            .unwrap();

        assert!(
            !resources.is_empty(),
            "Factory should have provided resources_db that received data"
        );
    }
}
