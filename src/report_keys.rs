use std::collections::HashMap;

use axum::{extract::Path, Extension, Json};
use serde::{Deserialize, Serialize};
use surrealdb::{
    engine::local::Db,
    sql::statements::{BeginStatement, CommitStatement},
    Surreal,
};
use tracing::info;

use crate::{
    auth::{AccountAuth, DashboardAuth},
    macros::*,
    report_key::{ReportKey, ReportKeyPublic, ReportKeyQueries},
    Result,
};

#[derive(Serialize)]
pub(crate) struct ListReportKeysResponse {
    report_api_keys: Vec<ReportKeyPublic>,
}

pub(crate) async fn list_report_keys(
    Extension(db): Extension<Surreal<Db>>,
) -> Result<Json<ListReportKeysResponse>> {
    let mut begin = BeginStatement::default();
    begin.readonly = true;

    let report_api_keys = db
        .query(begin)
        .list_report_keys_query()
        .query(CommitStatement::default())
        .await?
        .check()?
        .take::<Vec<ReportKey>>(0)?
        .into_iter()
        .map(ReportKeyPublic::from)
        .collect();

    Ok(Json(ListReportKeysResponse { report_api_keys }))
}

#[derive(Deserialize)]
pub(crate) struct CreateReportKeyRequest {
    description: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateReportKeyResponse {
    report_api_key: ReportKeyPublic,
    report_api_key_value: String,
}

pub(crate) async fn create_report_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(db): Extension<Surreal<Db>>,
    Json(req): Json<CreateReportKeyRequest>,
) -> Result<Json<CreateReportKeyResponse>> {
    let report_api_key = ReportKey::new(req.description, auth.principal().clone());
    let report_api_key_value = report_api_key
        .generate_value(
            auth.account_id()
                .expect("account ID should exist in auth context"),
        )
        .await?;

    let query = db
        .query(BeginStatement::default())
        .create_report_key_query(&report_api_key)
        .query(CommitStatement::default());

    info!(
        query = tracing::field::debug(&query),
        "Creating report key {report_key_id}",
        report_key_id = report_api_key.id()
    );

    let report_api_key = query
        .await?
        .check()?
        .take::<Option<ReportKey>>(0)?
        .expect("Create report API key query should return a report key instance");

    Ok(Json(CreateReportKeyResponse {
        report_api_key: ReportKeyPublic::from(report_api_key),
        report_api_key_value,
    }))
}

pub(crate) async fn revoke_report_key(
    Extension(auth): Extension<DashboardAuth>,
    Extension(db): Extension<Surreal<Db>>,
    Path(params): Path<HashMap<String, String>>,
) -> Result<Json<()>> {
    let Some(report_key_id_string) = params.get("report_key_id") else {
        bail!("Missing report_key_id");
    };

    let Ok(report_key_id) = report_key_id_string.parse() else {
        bad_request!("Invalid route key ID");
    };

    let report_key = db
        .query(BeginStatement::default())
        .revoke_report_key_query(report_key_id, auth.principal())
        .query(CommitStatement::default())
        .await?
        .check()?
        .take::<Option<ReportKey>>(0)?;

    if report_key.is_none() {
        not_found!("Report key not found");
    }

    Ok(Json(()))
}
