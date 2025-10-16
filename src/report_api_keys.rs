use std::collections::HashMap;

use axum::{Extension, Json, extract::Path};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use archodex_error::{anyhow::bail, bad_request, not_found};

use crate::{
    Result,
    account::AuthedAccount,
    auth::DashboardAuth,
    db::QueryCheckFirstRealError,
    report_api_key::{ReportApiKey, ReportApiKeyPublic, ReportApiKeyQueries},
};

#[derive(Serialize)]
pub(crate) struct ListReportApiKeysResponse {
    report_api_keys: Vec<ReportApiKeyPublic>,
}

#[instrument(err, skip_all)]
pub(crate) async fn list_report_api_keys(
    Extension(authed): Extension<AuthedAccount>,
) -> Result<Json<ListReportApiKeysResponse>> {
    let report_api_keys = authed
        .resources_db
        .list_report_api_keys_query()
        .await?
        .check_first_real_error()?
        .take::<Vec<ReportApiKey>>(0)?
        .into_iter()
        .map(ReportApiKeyPublic::from)
        .collect();

    Ok(Json(ListReportApiKeysResponse { report_api_keys }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateReportApiKeyRequest {
    description: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateReportApiKeyResponse {
    report_api_key: ReportApiKeyPublic,
    report_api_key_value: String,
}

#[instrument(err, skip(auth, authed))]
pub(crate) async fn create_report_api_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(authed): Extension<AuthedAccount>,
    Path(params): Path<HashMap<String, String>>,
    Json(req): Json<CreateReportApiKeyRequest>,
) -> Result<Json<CreateReportApiKeyResponse>> {
    let Some(account_id) = params.get("account_id") else {
        bail!("Missing account ID");
    };

    let report_api_key = ReportApiKey::new(req.description, auth.principal().clone());
    let report_api_key_value = report_api_key
        .generate_value(account_id, authed.account.salt().to_owned())
        .await?;

    let db = &authed.resources_db;

    let query = db.create_report_api_key_query(&report_api_key);

    let report_api_key = query
        .await?
        .check_first_real_error()?
        .take::<Option<ReportApiKey>>(0)?
        .expect("Create report API key query should return a report key instance");

    info!(
        report_api_key_id = report_api_key.id(),
        "Created Report API Key"
    );

    Ok(Json(CreateReportApiKeyResponse {
        report_api_key: ReportApiKeyPublic::from(report_api_key),
        report_api_key_value,
    }))
}

#[instrument(err, skip(auth, authed))]
pub(crate) async fn revoke_report_api_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(authed): Extension<AuthedAccount>,
    Path(params): Path<HashMap<String, String>>,
) -> Result<Json<()>> {
    let Some(report_api_key_id_string) = params.get("report_api_key_id") else {
        bail!("Missing report_api_key_id");
    };

    let Ok(report_api_key_id) = report_api_key_id_string.parse() else {
        bad_request!("Invalid route key ID");
    };

    let report_api_key = authed
        .resources_db
        .revoke_report_api_key_query(report_api_key_id, auth.principal())
        .await?
        .check_first_real_error()?
        .take::<Option<ReportApiKey>>(0)?;

    if report_api_key.is_none() {
        not_found!("Report key not found");
    }

    Ok(Json(()))
}
