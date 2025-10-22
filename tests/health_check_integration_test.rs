// Integration test for health check endpoint
//
// This test demonstrates the integration testing pattern with test routers
// that bypass authentication for simplicity.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt; // for oneshot()

#[tokio::test]
async fn test_health_endpoint() {
    // Create test router (no authentication required)
    let app = common::create_test_router();

    // Send HTTP request to /health endpoint
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Verify response status code
    assert_eq!(response.status(), StatusCode::OK);

    // Verify response body
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Ok");
}
