// Integration tests for /report endpoint with dependency-injected authentication
//
// These tests validate the complete request flow with trait-based dependency injection:
// - AuthProvider trait (FixedAuthProvider for tests, RealAuthProvider for production)
// - report_api_key_account middleware (account loading, key validation)
// - report::report handler (business logic)

mod common;
use common::init_test_env;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn test_middleware_rejects_nonexistent_account() {
    init_test_env();

    // Verify that middleware handles the case when account doesn't exist in database

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // FixedAuthProvider will attempt to authenticate with this account_id, but it won't exist
    let nonexistent_account_id = "account_does_not_exist";
    let key_id = 77777;
    let auth_provider = common::create_fixed_auth_provider(nonexistent_account_id, key_id);

    let app = common::create_test_router_with_state(accounts_db, resources_db, auth_provider);
    let report_payload = common::create_simple_test_report_request();

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
    init_test_env();

    // Verify middleware validates API keys in the injected resources database

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Create the account record but NOT the API key
    let account_id = "test_acc_no_key";
    let key_id = 88888;
    let _account = common::seed_test_account(&accounts_db, account_id, false).await;
    // NOTE: NOT calling seed_test_api_key() so key won't exist in database

    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);
    let app = common::create_test_router_with_state(accounts_db, resources_db, auth_provider);
    let report_payload = common::create_test_report_request();

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
    init_test_env();

    // Verify middleware correctly accesses the injected accounts_db

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Create test account record
    let account_id = "test_acc_456";
    let key_id = 99999;
    let _account = common::seed_test_account(&accounts_db, account_id, false).await;

    // Add test API key record
    common::seed_test_api_key(&resources_db, key_id).await;

    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    let app =
        common::create_test_router_with_state(accounts_db, resources_db.clone(), auth_provider);

    let report_payload = common::create_simple_test_report_request();

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
        "Middleware should load account from injected database and allow request"
    );

    // Verify resources were created in the resources db
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
            "Resources should be created in injected database"
        );
    }
}

#[tokio::test]
async fn test_middleware_uses_resources_db_factory() {
    init_test_env();

    // Verify the factory pattern works correctly with injected databases

    let accounts_db = common::create_test_accounts_db().await;
    let resources_db = common::create_test_resources_db().await;

    // Create test account record
    let account_id = "test_acc_789";
    let key_id = 99999;
    let _account = common::seed_test_account(&accounts_db, account_id, false).await;

    // Add test API key record
    common::seed_test_api_key(&resources_db, key_id).await;

    let auth_provider = common::create_fixed_auth_provider(account_id, key_id);

    let app =
        common::create_test_router_with_state(accounts_db, resources_db.clone(), auth_provider);

    let report_payload = common::create_simple_test_report_request();

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

    // Verify response against the database
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
