use axum::{middleware, routing::*, Router};
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowMethods, AllowOrigin, CorsLayer},
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{auth::auth, db::db, query, report, signup};

pub fn router() -> Router {
    Router::new()
        .route("/signup", post(signup::signup))
        .route("/report", post(report::report))
        .route("/query/:type", get(query::query))
        .layer(
            ServiceBuilder::new()
                .layer(
                    CorsLayer::new()
                        .allow_methods(AllowMethods::any())
                        .allow_origin(AllowOrigin::any()),
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
