use axum::{Extension, Json};
use serde::Serialize;

use crate::{auth::Principal, Result};

#[derive(Serialize)]
pub(crate) struct ListAccountsResponse {
    accounts: Vec<u64>,
}

pub(crate) async fn list_accounts(
    Extension(principal): Extension<Principal>,
) -> Result<Json<ListAccountsResponse>> {
    let accounts = if let Some(account_id) = principal.account_id() {
        vec![account_id]
    } else {
        vec![]
    };

    Ok(Json(ListAccountsResponse { accounts }))
}
