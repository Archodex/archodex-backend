use axum::{http::header::CONTENT_TYPE, middleware, routing::*, Router};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowMethods, AllowOrigin, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{auth::auth, db::db, principal_chain, query, report, signup};

pub fn router() -> Router {
    Router::new()
        .route("/signup", post(signup::signup))
        .route("/report", post(report::report))
        .route("/query/:type", get(query::query))
        .route("/principal_chain", get(principal_chain::get))
        .layer(
            ServiceBuilder::new()
                .layer(
                    CorsLayer::new()
                        .allow_methods(AllowMethods::any())
                        .allow_origin(AllowOrigin::any())
                        .allow_headers([CONTENT_TYPE]),
                )
                .layer(middleware::from_fn(auth))
                .layer(middleware::from_fn(db)),
        )
        .route("/health", get(|| async { "Ok" }))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}
