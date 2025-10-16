use std::time::Duration;

use axum::{
    Router,
    http::{
        HeaderValue,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    routing::{delete, get, post},
};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowMethods, AllowOrigin, CorsLayer},
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span, error_span};
use uuid::Uuid;

use crate::{
    accounts,
    auth::DashboardAuth,
    db::{create_production_state, dashboard_auth_account, report_api_key_account},
    env::Env,
    principal_chain, query, report, report_api_keys, resource,
    state::AppState,
};

/// Creates router with dependency-injected state
///
/// This function is pub for use by tests. Production code should use `router()`.
///
/// # Arguments
/// * `state` - Application state with injected database connections and factories
///
/// # Panics
///
/// Will panic if `Env::archodex_domain()` is not a valid domain.
pub fn create_router_with_state(state: AppState) -> Router {
    let cors_layer = CorsLayer::new()
        .allow_methods(AllowMethods::mirror_request())
        .allow_origin(AllowOrigin::list([
            HeaderValue::from_str(&format!("https://app.{}", Env::archodex_domain()))
                .expect("Failed to parse archodex domain as HeaderValue"),
            HeaderValue::from_str("http://localhost:5173")
                .expect("Failed to parse localhost as HeaderValue"),
        ]))
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let unauthed_router = Router::new().route("/health", get(|| async { "Ok" }));

    let dashboard_authed_router = Router::new()
        .nest(
            "/account/:account_id",
            Router::new()
                .route(
                    "/resource/set_environments",
                    post(resource::set_environments),
                )
                .route("/query/:type", get(query::query))
                .route("/principal_chain", get(principal_chain::get))
                .route(
                    "/report_api_keys",
                    get(report_api_keys::list_report_api_keys),
                )
                .route(
                    "/report_api_keys",
                    post(report_api_keys::create_report_api_key),
                )
                .route(
                    "/report_api_key/:report_api_key_id",
                    delete(report_api_keys::revoke_report_api_key),
                )
                .route("/", delete(accounts::delete_account)),
        )
        // Account loading middleware (runs second, inner layer)
        .layer(ServiceBuilder::new().layer(middleware::from_fn_with_state(
            state.clone(),
            dashboard_auth_account,
        )))
        .route("/accounts", get(accounts::list_accounts))
        .route("/accounts", post(accounts::create_account))
        // Auth middleware (runs first, outer layer)
        .layer(ServiceBuilder::new().layer(middleware::from_fn(DashboardAuth::authenticate)))
        .layer(cors_layer.clone());

    let report_api_key_authed_router = Router::new()
        .route("/report", post(report::report))
        // Authentication and account loading middleware
        .layer(ServiceBuilder::new().layer(middleware::from_fn_with_state(
            state.clone(),
            report_api_key_account,
        )));

    let default_on_response_trace_handler = DefaultOnResponse::new().level(Level::INFO);

    unauthed_router
        .merge(dashboard_authed_router)
        .merge(report_api_key_authed_router)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    use tracing::field::Empty;

                    let span = error_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        auth = Empty,
                        request_id = %Uuid::now_v7(),
                        "X-Request-ID" = Empty,
                        version = ?request.version(),
                    );

                    if let Some(x_request_id) = request.headers().get("X-Request-ID") {
                        span.record("X-Request-ID", tracing::field::debug(x_request_id));
                    }

                    span
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    |response: &axum::http::Response<_>, latency: Duration, span: &Span| {
                        use tower_http::trace::OnResponse;

                        // Skip logging 5xx responses. These are already logged by the default on_failure handler.
                        if !response.status().is_server_error() {
                            default_on_response_trace_handler.on_response(response, latency, span);
                        }
                    },
                ),
        )
        .with_state(state)
}

/// Creates router with production state
///
/// This is the main entry point for production code. It initializes the
/// application state with global database connections and creates the router.
///
/// # Panics
///
/// Will panic if `Env::archodex_domain()` is not a valid domain.
pub async fn router() -> Router {
    let state = create_production_state().await;

    create_router_with_state(state)
}
