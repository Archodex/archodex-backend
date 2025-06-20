use axum::{http::header::CONTENT_TYPE, middleware, routing::*, Router};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowCredentials, AllowMethods, AllowOrigin, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{
    accounts,
    auth::{dashboard_auth, report_api_key_auth, DashboardAuth, ReportApiKeyAuth},
    db::db,
    oauth2, principal_chain, query, report, report_api_keys, resource,
};

pub fn router() -> Router {
    let cors_layer = CorsLayer::new()
        .allow_methods(AllowMethods::mirror_request())
        .allow_origin(AllowOrigin::predicate(|origin, _request_parts| {
            origin == "http://localhost:5173"
                || origin.as_bytes().ends_with(b".archodex.com")
                || origin.as_bytes().ends_with(b".dev.servicearch.com")
        }))
        .allow_headers([CONTENT_TYPE])
        .allow_credentials(AllowCredentials::yes());

    let unauthed_router = Router::new()
        .route("/oauth2/token", post(oauth2::refresh_token_remote))
        .route("/oauth2/token/local", post(oauth2::refresh_token_local))
        .route("/oauth2/revoke", post(oauth2::revoke_token))
        .layer(cors_layer.clone())
        .route("/oauth2/idpresponse", get(oauth2::idp_response_remote))
        .route("/oauth2/idpresponse/local", get(oauth2::idp_response_local))
        .route("/health", get(|| async { "Ok" }));

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
                ),
        )
        .layer(ServiceBuilder::new().layer(middleware::from_fn(db::<DashboardAuth>)))
        .route("/accounts", get(accounts::list_accounts))
        .route("/accounts", post(accounts::create_account))
        .layer(ServiceBuilder::new().layer(middleware::from_fn(dashboard_auth)))
        .layer(cors_layer.clone());

    let report_api_key_authed_router = Router::new()
        .route("/report", post(report::report))
        .layer(ServiceBuilder::new().layer(middleware::from_fn(db::<ReportApiKeyAuth>)))
        .layer(ServiceBuilder::new().layer(middleware::from_fn(report_api_key_auth)));

    unauthed_router
        .merge(dashboard_authed_router)
        .merge(report_api_key_authed_router)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}
