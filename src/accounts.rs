use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use surrealdb::sql::statements::{BeginStatement, CommitStatement};
use tracing::instrument;

use archodex_error::anyhow::Context as _;

use crate::{
    Result,
    account::{Account, AccountPublic, AccountQueries},
    auth::DashboardAuth,
    db::{QueryCheckFirstRealError, accounts_db},
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

#[instrument(err, skip(auth))]
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
#[instrument(err, skip_all)]
pub(crate) async fn create_local_account(
    auth: DashboardAuth,
    req: CreateAccountRequest,
) -> Result<Json<AccountPublic>> {
    verify_no_local_accounts_exist().await?;

    let principal = auth.principal();
    principal.ensure_user_record_exists().await?;

    let account = Account::new(req.account_id, principal.clone())
        .await
        .context("Failed to create new account")?;

    accounts_db()
        .await?
        .query(BeginStatement::default())
        .create_account_query(&account, principal)
        .add_account_access_for_user(&account, principal)
        .query(CommitStatement::default())
        .await
        .context("Failed to submit query to create new account record in accounts database")?
        .check_first_real_error()
        .context("Failed to create new account record in accounts database")?;

    Ok(Json(account.into()))
}

#[cfg(not(feature = "archodex-com"))]
#[instrument(err, skip_all)]
async fn verify_no_local_accounts_exist() -> Result<()> {
    use archodex_error::{anyhow::anyhow, conflict};

    let local_account_exists: bool = accounts_db()
        .await?
        .query("RETURN COUNT(SELECT id FROM account WHERE deleted_at IS NONE LIMIT 1) > 0")
        .await?
        .check_first_real_error()?
        .take::<Option<bool>>(0)
        .context("Failed to retrieve local accounts count")?
        .ok_or_else(|| anyhow!("Failed to retrieve local accounts count"))?;

    if local_account_exists {
        conflict!("An account already exists for this local backend");
    }

    Ok(())
}

#[cfg(feature = "archodex-com")]
pub(crate) async fn create_archodex_com_account(
    auth: DashboardAuth,
    req: CreateAccountRequest,
) -> Result<Json<AccountPublic>> {
    use crate::env::Env;

    let endpoint = if let Some(endpoint) = req.endpoint {
        endpoint
    } else {
        Env::endpoint().to_string()
    };

    let accounts_db = accounts_db().await?;

    let principal = auth.principal();
    principal.ensure_user_record_exists().await?;

    let next_account_id = principal.next_account_id().await?;

    let account = Account::new(endpoint, next_account_id, principal.clone())
        .await
        .context("Failed to create new account")?;

    accounts_db
        .query(BeginStatement::default())
        .create_account_query(&account, principal)
        .add_account_access_for_user(&account, principal)
        .query(CommitStatement::default())
        .await
        .context("Failed to commit account creation transaction")?
        .check_first_real_error()
        .context("Failed to create new account record in accounts database")?;

    Ok(Json(account.into()))
}

#[instrument(err)]
pub(crate) async fn delete_account(
    Extension(auth): Extension<DashboardAuth>,
    Extension(account): Extension<Account>,
) -> Result<()> {
    auth.principal().ensure_user_record_exists().await?;

    let db = accounts_db().await?;

    #[cfg(not(feature = "archodex-com"))]
    {
        db.query(BeginStatement::default())
            .query("REMOVE DATABASE resources")
            .query(CommitStatement::default())
            .await
            .context("Failed to submit query to delete data in resources database")?
            .check_first_real_error()
            .context("Failed to delete data in resources database")?;
    }

    #[cfg(feature = "archodex-com")]
    if let Some(service_data_surrealdb_url) = account.service_data_surrealdb_url() {
        archodex_com::delete_account_service_database(service_data_surrealdb_url, account.id())
            .await?;
    }

    db.query(BeginStatement::default())
        .delete_account_query(&account, auth.principal())
        .query(CommitStatement::default())
        .await
        .context("Failed to submit query to delete account record in accounts database")?
        .check_first_real_error()
        .context("Failed to delete account record in accounts database")?;

    Ok(())
}
