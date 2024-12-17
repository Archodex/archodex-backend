use aes_gcm::{
    aead::{self, Aead},
    AeadCore, Aes128Gcm, KeyInit,
};
use anyhow::Context;
use base64::prelude::*;
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{env::Env, macros::*, next_binding, surrealdb_deserializers, user::User};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ReportKey {
    #[serde(deserialize_with = "surrealdb_deserializers::u32::deserialize")]
    id: u32,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
    created_by: User,
    #[allow(dead_code)]
    revoked_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    revoked_by: Option<User>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReportKeyPublic {
    #[serde(deserialize_with = "surrealdb_deserializers::u32::deserialize")]
    id: u32,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
}

impl From<ReportKey> for ReportKeyPublic {
    fn from(record: ReportKey) -> Self {
        Self {
            id: record.id,
            description: record.description,
            created_at: record.created_at,
        }
    }
}

impl ReportKey {
    pub(crate) fn new(description: Option<String>, created_by: User) -> Self {
        Self {
            id: rand::thread_rng().gen_range::<u32, _>(100000..=999999),
            description,
            created_at: None,
            created_by,
            revoked_at: None,
            revoked_by: None,
        }
    }

    pub(crate) fn id(&self) -> u32 {
        self.id
    }

    pub(crate) async fn generate_value(&self, account_id: &str) -> anyhow::Result<String> {
        let cipher = Aes128Gcm::new(Env::api_key_kms_data_key().await);
        let nonce = Aes128Gcm::generate_nonce(&mut rand::rngs::OsRng);
        let aad = format!("key_id={};endpoint={}", self.id, Env::endpoint());
        let plaintext_msg = format!("account_id={account_id}");
        let encrypted_account_id = cipher
            .encrypt(
                &nonce,
                aead::Payload {
                    msg: plaintext_msg.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|err| anyhow!("Failed to encrypt account ID: {err}"))?;

        let mut report_api_key_value = Vec::<u8>::new();
        report_api_key_value.push(Env::endpoint().len() as u8);
        report_api_key_value.extend_from_slice(Env::endpoint().as_bytes());
        report_api_key_value.push(nonce.len() as u8);
        report_api_key_value.extend_from_slice(nonce.as_slice());
        report_api_key_value.extend_from_slice(&encrypted_account_id);

        Ok(format!(
            "archodex_report_key_{}_{}",
            self.id,
            BASE64_STANDARD.encode(&report_api_key_value)
        ))
    }

    // This method validates a report key value contains the correct endpoint and returns the account and key IDs. The
    // caller must still validate the key ID exists for the account and has not been revoked.
    pub(crate) async fn validate_value(report_key_value: &str) -> anyhow::Result<(String, u32)> {
        let Some(key_id) = report_key_value.strip_prefix("archodex_report_key_") else {
            bail!("Invalid report key value: Missing prefix");
        };

        let key_id_value = key_id.splitn(2, '_').collect::<Vec<_>>();

        let [key_id, value] = key_id_value[..] else {
            bail!("Invalid report key value: Invalid format");
        };

        let key_id = key_id
            .parse::<u32>()
            .context("Invalid report key value: Key ID is not a number")?;

        ensure!(
            key_id >= 100000 && key_id <= 999999,
            "Invalid report key value: Key ID is out of range"
        );

        let value = BASE64_STANDARD
            .decode(value)
            .context("Failed to base64 decode report key value")?;

        ensure!(
            value.len() > 0,
            "Invalid report key value: Missing endpoint length"
        );

        let endpoint_len = value[0] as usize;

        ensure!(
            value.len() > 1 + endpoint_len,
            "Invalid report key value: Invalid endpoint length"
        );

        let endpoint = std::str::from_utf8(&value[1..=endpoint_len])
            .context("Invalid report key value: Non-UTF-8 endpoint")?;

        ensure!(
            endpoint == Env::endpoint(),
            "Invalid report key value: Incorrect endpoint"
        );

        ensure!(
            value.len() > 1 + endpoint_len + 1,
            "Invalid report key value: Missing nonce length"
        );

        let nonce_len = value[1 + endpoint_len] as usize;

        ensure!(
            value.len() > 1 + endpoint_len + nonce_len + 1,
            "Invalid report key value: Invalid nonce length"
        );

        let nonce = aead::Nonce::<Aes128Gcm>::from_slice(
            &value[1 + endpoint_len + 1..1 + endpoint_len + 1 + nonce_len],
        );
        let encrypted_message = &value[1 + endpoint_len + 1 + nonce_len..];

        let cipher = Aes128Gcm::new(Env::api_key_kms_data_key().await);
        let aad = format!("key_id={key_id};endpoint={endpoint}");
        let decrypted_message = cipher
            .decrypt(
                nonce,
                aead::Payload {
                    msg: encrypted_message,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|err| {
                anyhow!("Invalid report key value: Failed to decrypt account ID: {err}")
            })?;

        let decrypted_message = std::str::from_utf8(&decrypted_message)
            .context("Invalid report key value: Non-UTF-8 account ID")?;

        let Some(account_id) = decrypted_message.strip_prefix("account_id=") else {
            bail!("Invalid report key value: Missing account ID prefix")
        };

        if account_id.parse::<u32>().is_err() {
            bail!("Invalid report key value: Account ID is not valid");
        }

        Ok((account_id.to_string(), key_id))
    }
}

pub(crate) trait ReportKeyQueries<'r, C: surrealdb::Connection> {
    fn list_report_keys_query(self) -> surrealdb::method::Query<'r, C>;
    fn create_report_key_query(self, report_key: &ReportKey) -> surrealdb::method::Query<'r, C>;
    fn revoke_report_key_query(
        self,
        report_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn report_key_is_valid_query(self, id: u32) -> surrealdb::method::Query<'r, C>;
    type ReportKeyIsValidQueryResponse;
}

#[derive(Deserialize)]
pub(crate) struct ReportKeyIsValidQueryResponse {
    valid: bool,
}

impl ReportKeyIsValidQueryResponse {
    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }
}

impl<'r, C: surrealdb::Connection> ReportKeyQueries<'r, C> for surrealdb::method::Query<'r, C> {
    fn list_report_keys_query(self) -> surrealdb::method::Query<'r, C> {
        self.query("SELECT * FROM report_key WHERE type::is::none(revoked_at)")
    }

    fn create_report_key_query(self, report_key: &ReportKey) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();
        let description_binding = next_binding();
        let created_by_binding = next_binding();

        self
            .query(format!("CREATE ${report_key_binding} CONTENT {{ description: ${description_binding}, created_by: ${created_by_binding} }}"))
            .bind((report_key_binding, surrealdb::sql::Thing::from(report_key)))
            .bind((description_binding, report_key.description.to_owned()))
            .bind((created_by_binding, surrealdb::sql::Thing::from(&report_key.created_by)))
    }

    fn revoke_report_key_query(
        self,
        report_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();
        let revoked_by_binding = next_binding();

        self.query(
            format!("UPDATE ${report_key_binding} SET revoked_at = time::now(), revoked_by = ${revoked_by_binding} WHERE revoked_at IS NONE"),
        )
        .bind((
            report_key_binding,
            surrealdb::sql::Thing::from((
                "report_key",
                surrealdb::sql::Id::from(report_key_id as i64),
            )),
        ))
        .bind((revoked_by_binding, surrealdb::sql::Thing::from(revoked_by)))
    }

    fn report_key_is_valid_query(self, report_key_id: u32) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();

        self.query(format!(
            "SELECT type::is::none(revoked_at) AS valid FROM ${report_key_binding}"
        ))
        .bind((
            report_key_binding,
            surrealdb::sql::Thing::from((
                "report_key",
                surrealdb::sql::Id::from(report_key_id as i64),
            )),
        ))
    }

    type ReportKeyIsValidQueryResponse = ReportKeyIsValidQueryResponse;
}

impl From<&ReportKey> for surrealdb::sql::Thing {
    fn from(report_key: &ReportKey) -> Self {
        Self::from((
            "report_key",
            surrealdb::sql::Id::Number(report_key.id as i64),
        ))
    }
}
