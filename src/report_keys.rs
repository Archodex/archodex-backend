use std::collections::HashMap;

use aes_gcm::{
    aead::{self, Aead},
    AeadCore, Aes128Gcm, KeyInit,
};
use axum::{extract::Path, Extension, Json};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use surrealdb::{
    engine::local::Db,
    sql::statements::{BeginStatement, CommitStatement},
    Surreal,
};

use crate::{
    auth::Auth,
    env::Env,
    macros::*,
    report_key::{ReportKey, ReportKeyPublic, ReportKeyQueries},
    Result,
};

#[derive(Serialize)]
pub(crate) struct ListReportKeysResponse {
    report_keys: Vec<ReportKeyPublic>,
}

pub(crate) async fn list_report_keys(
    Extension(db): Extension<Surreal<Db>>,
) -> Result<Json<ListReportKeysResponse>> {
    let mut begin = BeginStatement::default();
    begin.readonly = true;

    let report_keys = db
        .query(begin)
        .list_report_keys_query()
        .await?
        .check()?
        .take::<Vec<ReportKey>>(0)?
        .into_iter()
        .map(ReportKeyPublic::from)
        .collect();

    Ok(Json(ListReportKeysResponse { report_keys }))
}

#[derive(Deserialize)]
pub(crate) struct CreateReportKeyRequest {
    description: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateReportKeyResponse {
    report_key_value: String,
}

pub(crate) async fn create_report_key(
    Extension(auth): Extension<Auth>,
    Extension(db): Extension<Surreal<Db>>,
    Json(req): Json<CreateReportKeyRequest>,
) -> Result<Json<CreateReportKeyResponse>> {
    let report_key = ReportKey::new(req.description, auth.principal().clone());

    db.query(BeginStatement::default())
        .create_report_key_query(&report_key)
        .query(CommitStatement::default())
        .await?
        .check()?;

    let cipher = Aes128Gcm::new(Env::api_key_kms_data_key().await);
    let nonce = Aes128Gcm::generate_nonce(&mut rand::rngs::OsRng);
    let aad = format!("endpoint={}", Env::endpoint());
    let encrypted_account_id = cipher
        .encrypt(
            &nonce,
            aead::Payload {
                msg: auth
                    .account_id()
                    .expect("account ID should exist in auth context")
                    .as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|err| anyhow!("Failed to encrypt account ID: {err}"))?;

    let mut report_key_value = Vec::<u8>::new();
    report_key_value.push(Env::endpoint().len() as u8);
    report_key_value.extend_from_slice(Env::endpoint().as_bytes());
    report_key_value.push(nonce.len() as u8);
    report_key_value.extend_from_slice(nonce.as_slice());
    report_key_value.extend_from_slice(&encrypted_account_id);

    let report_key_value = BASE64_STANDARD.encode(&report_key_value);

    Ok(Json(CreateReportKeyResponse { report_key_value }))
}

pub(crate) async fn revoke_report_key(
    Extension(auth): Extension<Auth>,
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
