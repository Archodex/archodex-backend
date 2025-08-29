use std::collections::HashMap;

use axum::{Extension, Json, extract::Path};
use serde::{Deserialize, Serialize};
use surrealdb::{
    Surreal,
    engine::any::Any,
    sql::statements::{BeginStatement, CommitStatement},
};
use tracing::info;

use archodex_error::{
    anyhow::{Context as _, anyhow, bail},
    bad_request, not_found,
};

use crate::{
    Result,
    account::{Account, AccountQueries},
    auth::{AccountAuth, DashboardAuth},
    db::{BeginReadonlyStatement, QueryCheckFirstRealError, accounts_db},
    report_api_key::{ReportApiKey, ReportApiKeyPublic, ReportApiKeyQueries},
};

#[derive(Serialize)]
pub(crate) struct ListReportApiKeysResponse {
    report_api_keys: Vec<ReportApiKeyPublic>,
}

pub(crate) async fn list_report_api_keys(
    Extension(db): Extension<Surreal<Any>>,
) -> Result<Json<ListReportApiKeysResponse>> {
    let report_api_keys = db
        .query(BeginReadonlyStatement)
        .list_report_api_keys_query()
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?
        .take::<Vec<ReportApiKey>>(0)?
        .into_iter()
        .map(ReportApiKeyPublic::from)
        .collect();

    Ok(Json(ListReportApiKeysResponse { report_api_keys }))
}

#[derive(Deserialize)]
pub(crate) struct CreateReportApiKeyRequest {
    description: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateReportApiKeyResponse {
    report_api_key: ReportApiKeyPublic,
    report_api_key_value: String,
}

pub(crate) async fn create_report_api_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(db): Extension<Surreal<Any>>,
    Json(req): Json<CreateReportApiKeyRequest>,
) -> Result<Json<CreateReportApiKeyResponse>> {
    let account_id = auth
        .account_id()
        .expect("account ID should exist in auth context");

    let account = accounts_db()
        .await?
        .query(BeginReadonlyStatement)
        .get_account_by_id(account_id.to_owned())
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?
        .take::<Option<Account>>(0)
        .with_context(|| format!("Failed to get record for account ID {account_id:?}"))?
        .ok_or_else(|| anyhow!("Account record not found for ID {account_id:?}"))?;

    let report_api_key = ReportApiKey::new(req.description, auth.principal().clone());
    let report_api_key_value = report_api_key
        .generate_value(account_id, account.salt().to_owned())
        .await?;

    let query = db
        .query(BeginStatement::default())
        .create_report_api_key_query(&report_api_key)
        .query(CommitStatement::default());

    info!(
        query = tracing::field::debug(&query),
        "Creating report key {report_api_key_id}",
        report_api_key_id = report_api_key.id()
    );

    let report_api_key = query
        .await?
        .check_first_real_error()?
        .take::<Option<ReportApiKey>>(0)?
        .expect("Create report API key query should return a report key instance");

    Ok(Json(CreateReportApiKeyResponse {
        report_api_key: ReportApiKeyPublic::from(report_api_key),
        report_api_key_value,
    }))
}

pub(crate) async fn revoke_report_api_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(db): Extension<Surreal<Any>>,
    Path(params): Path<HashMap<String, String>>,
) -> Result<Json<()>> {
    let Some(report_api_key_id_string) = params.get("report_api_key_id") else {
        bail!("Missing report_api_key_id");
    };

    let Ok(report_api_key_id) = report_api_key_id_string.parse() else {
        bad_request!("Invalid route key ID");
    };

    let report_api_key = db
        .query(BeginStatement::default())
        .revoke_report_api_key_query(report_api_key_id, auth.principal())
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?
        .take::<Option<ReportApiKey>>(0)?;

    if report_api_key.is_none() {
        not_found!("Report key not found");
    }

    Ok(Json(()))
}
