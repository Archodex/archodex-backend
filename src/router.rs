use axum::{http::header::CONTENT_TYPE, middleware, routing::*, Router};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowCredentials, AllowMethods, AllowOrigin, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{auth::auth, db::db, oauth2, principal_chain, query, report, signup};

pub fn router() -> Router {
    Router::new()
        .route("/signup", post(signup::signup))
        .route("/report", post(report::report))
        .route("/query/:type", get(query::query))
        .route("/principal_chain", get(principal_chain::get))
        .layer(
            ServiceBuilder::new()
                .layer(middleware::from_fn(auth))
                .layer(middleware::from_fn(db)),
        )
        .route("/oauth2/token", post(oauth2::refresh_token))
        .route("/oauth2/revoke", post(oauth2::revoke_token))
        .layer(
            CorsLayer::new()
                .allow_methods(AllowMethods::mirror_request())
                .allow_origin(AllowOrigin::predicate(|origin, _request_parts| {
                    origin == "http://localhost:5173"
                        || origin.as_bytes().ends_with(b".archodex.com")
                        || origin.as_bytes().ends_with(b".dev.servicearch.com")
                }))
                .allow_headers([CONTENT_TYPE])
                .allow_credentials(AllowCredentials::yes()),
        )
        .route("/oauth2/idpresponse", get(oauth2::idp_response))
        .route("/health", get(|| async { "Ok" }))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}
