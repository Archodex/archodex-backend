use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use surrealdb::sql::statements::{BeginStatement, CommitStatement};

#[cfg(not(feature = "archodex-com"))]
use surrealdb::{Surreal, engine::any::Any};

use archodex_error::{anyhow::Context as _, conflict};

use crate::{
    Result,
    account::{Account, AccountPublic, AccountQueries},
    auth::DashboardAuth,
    db::{QueryCheckFirstRealError, accounts_db},
    env::Env,
};

#[derive(Serialize)]
pub(crate) struct ListAccountsResponse {
    accounts: Vec<AccountPublic>,
}

pub(crate) async fn list_accounts(
    Extension(auth): Extension<DashboardAuth>,
) -> Result<Json<ListAccountsResponse>> {
    let accounts = auth
        .principal()
        .list_accounts()
        .await?
        .into_iter()
        .map(AccountPublic::from)
        .collect();

    Ok(Json(ListAccountsResponse { accounts }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateAccountRequest {
    #[cfg(not(feature = "archodex-com"))]
    account_id: String,
    #[cfg(feature = "archodex-com")]
    endpoint: Option<String>,
}

pub(crate) async fn create_account(
    Extension(auth): Extension<DashboardAuth>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<AccountPublic>> {
    #[cfg(not(feature = "archodex-com"))]
    {
        create_local_account(auth, req).await
    }

    #[cfg(feature = "archodex-com")]
    {
        create_archodex_com_account(auth, req).await
    }
}

#[cfg(not(feature = "archodex-com"))]
pub(crate) async fn create_local_account(
    auth: DashboardAuth,
    req: CreateAccountRequest,
) -> Result<Json<AccountPublic>> {
    let endpoint = Env::endpoint();

    let accounts_db = accounts_db().await?;

    verify_no_local_accounts_exist(&accounts_db).await?;

    let principal = auth.principal();
    principal.ensure_user_record_exists().await?;

    let account = Account::new(endpoint.to_string(), Some(req.account_id))
        .await
        .context("Failed to create new account")?;

    accounts_db
        .query(BeginStatement::default())
        .create_account_query(&account)
        .add_account_access_for_user(&account, principal)
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?;

    Ok(Json(account.into()))
}

#[cfg(not(feature = "archodex-com"))]
async fn verify_no_local_accounts_exist(accounts_db: &Surreal<Any>) -> Result<()> {
    #[cfg(not(feature = "archodex-com"))]
    #[derive(Deserialize, PartialEq)]
    struct AccountsCount {
        count: u64,
    }

    let accounts_count_results = accounts_db
        .query("SELECT COUNT() FROM ONLY account")
        .await?
        .check_first_real_error()?
        .take::<Option<AccountsCount>>(0)
        .context("Failed to retrieve local accounts count")?;

    if accounts_count_results.is_some()
        && accounts_count_results != Some(AccountsCount { count: 0 })
    {
        conflict!("An account already exists for this local backend");
    }

    Ok(())
}

#[cfg(feature = "archodex-com")]
pub(crate) async fn create_archodex_com_account(
    auth: DashboardAuth,
    req: CreateAccountRequest,
) -> Result<Json<AccountPublic>> {
    let endpoint = if let Some(endpoint) = req.endpoint {
        endpoint
    } else {
        Env::endpoint().to_string()
    };

    let accounts_db = accounts_db().await?;

    let principal = auth.principal();
    principal.ensure_user_record_exists().await?;

    // TODO: Multi-account support
    if principal.has_user_account().await? {
        conflict!("User already has an account");
    }

    let account = Account::new(endpoint, None)
        .await
        .context("Failed to create new account")?;

    accounts_db
        .query(BeginStatement::default())
        .create_account_query(&account)
        .add_account_access_for_user(&account, principal)
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?;

    Ok(Json(account.into()))
}
