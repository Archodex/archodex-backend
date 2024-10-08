use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};

pub(crate) const DEMO_ACCOUNT_ID: &str = "4070221500";

#[derive(Clone)]
pub(crate) struct Principal {
    account_id: String,
}

impl Principal {
    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }
}

pub(crate) async fn auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    req.extensions_mut().insert(Principal {
        account_id: DEMO_ACCOUNT_ID.to_owned(),
    });

    Ok(next.run(req).await)
}
