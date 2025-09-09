pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/archodex.report_api_key.rs"));
}

use aes_gcm::{
    AeadCore, Aes128Gcm, KeyInit,
    aead::{self, Aead},
};
use base64::prelude::*;
use chrono::{DateTime, Utc};
use prost::Message;
use rand::Rng;
use serde::{Deserialize, Serialize};

use archodex_error::anyhow::{self, Context as _, anyhow, bail, ensure};

use crate::{env::Env, next_binding, surrealdb_deserializers, user::User};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ReportApiKey {
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
pub(crate) struct ReportApiKeyPublic {
    #[serde(deserialize_with = "surrealdb_deserializers::u32::deserialize")]
    id: u32,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
}

impl From<ReportApiKey> for ReportApiKeyPublic {
    fn from(record: ReportApiKey) -> Self {
        Self {
            id: record.id,
            description: record.description,
            created_at: record.created_at,
        }
    }
}

impl ReportApiKey {
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

    pub(crate) async fn generate_value(
        &self,
        account_id: &str,
        account_salt: Vec<u8>,
    ) -> anyhow::Result<String> {
        let cipher = Aes128Gcm::new(Env::api_private_key().await);
        let nonce = Aes128Gcm::generate_nonce(&mut rand::rngs::OsRng);

        let message = proto::ReportApiKeyEncryptedContents {
            account_id: account_id.parse::<u32>().context("Invalid account ID")?,
        };

        let aad = proto::ReportApiKeyEncryptedAad {
            key_id: self.id,
            endpoint: Env::endpoint().to_owned(),
            account_salt: account_salt.to_owned(),
        };

        let encrypted_account_id = cipher
            .encrypt(
                &nonce,
                aead::Payload {
                    msg: &message.encode_to_vec(),
                    aad: &aad.encode_to_vec(),
                },
            )
            .map_err(|err| anyhow!("Failed to encrypt account ID: {err}"))?;

        let report_api_key = proto::ReportApiKey {
            report_api_key_version: 1,
            endpoint: Env::endpoint().to_owned(),
            account_salt,
            nonce: nonce.as_slice().to_vec(),
            encrypted_contents: encrypted_account_id,
        };

        Ok(format!(
            "archodex_report_api_key_{}_{}",
            self.id,
            BASE64_STANDARD.encode(report_api_key.encode_to_vec())
        ))
    }

    // This method validates a report key value contains the correct endpoint and returns the account and key IDs. The
    // caller must still validate the key ID exists for the account and has not been revoked.
    pub(crate) async fn validate_value(
        report_api_key_value: &str,
    ) -> anyhow::Result<(String, u32)> {
        let Some(key_id) = report_api_key_value.strip_prefix("archodex_report_api_key_") else {
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
            (100000..=999999).contains(&key_id),
            "Invalid report key value: Key ID is out of range"
        );

        let value = BASE64_STANDARD
            .decode(value)
            .context("Failed to base64 decode report key value")?;

        ensure!(
            !value.is_empty(),
            "Invalid report key value: Missing endpoint length"
        );

        let value = proto::ReportApiKey::decode(value.as_slice())
            .context("Invalid report key value: Failed to decode report key value as protobuf")?;

        ensure!(
            value.endpoint == Env::endpoint(),
            "Invalid report key value: Incorrect endpoint"
        );

        ensure!(
            value.account_salt.len() == 16,
            "Invalid report key value: Account salt is not 16 bytes long"
        );

        let nonce = aead::Nonce::<Aes128Gcm>::from_slice(&value.nonce);
        let cipher = Aes128Gcm::new(Env::api_private_key().await);

        let aad = proto::ReportApiKeyEncryptedAad {
            key_id,
            endpoint: value.endpoint,
            account_salt: value.account_salt,
        };

        let decrypted_message = cipher
            .decrypt(
                nonce,
                aead::Payload {
                    msg: &value.encrypted_contents,
                    aad: &aad.encode_to_vec(),
                },
            )
            .map_err(|err| {
                anyhow!("Invalid report key value: Failed to decrypt encrypted contents: {err}")
            })?;

        let encrypted_contents = proto::ReportApiKeyEncryptedContents::decode(
            decrypted_message.as_slice(),
        )
        .context("Invalid report key value: Failed to decode decrypted message as protobuf")?;

        ensure!(
            encrypted_contents.account_id >= 1000000000,
            "Invalid report key value: Account ID is out of range"
        );

        Ok((encrypted_contents.account_id.to_string(), key_id))
    }
}

pub(crate) trait ReportApiKeyQueries<'r, C: surrealdb::Connection> {
    fn list_report_api_keys_query(self) -> surrealdb::method::Query<'r, C>;
    fn create_report_api_key_query(
        self,
        report_api_key: &ReportApiKey,
    ) -> surrealdb::method::Query<'r, C>;
    fn revoke_report_api_key_query(
        self,
        report_api_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn report_api_key_is_valid_query(self, id: u32) -> surrealdb::method::Query<'r, C>;
    type ReportApiKeyIsValidQueryResponse;
}

#[derive(Deserialize)]
pub(crate) struct ReportApiKeyIsValidQueryResponse {
    valid: bool,
}

impl ReportApiKeyIsValidQueryResponse {
    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }
}

impl<'r, C: surrealdb::Connection> ReportApiKeyQueries<'r, C> for surrealdb::method::Query<'r, C> {
    fn list_report_api_keys_query(self) -> surrealdb::method::Query<'r, C> {
        self.query("SELECT * FROM report_api_key WHERE type::is::none(revoked_at)")
    }

    fn create_report_api_key_query(
        self,
        report_api_key: &ReportApiKey,
    ) -> surrealdb::method::Query<'r, C> {
        let report_api_key_binding = next_binding();
        let description_binding = next_binding();
        let created_by_binding = next_binding();

        self
            .query(format!("CREATE ${report_api_key_binding} CONTENT {{ description: ${description_binding}, created_by: ${created_by_binding} }}"))
            .bind((report_api_key_binding, surrealdb::sql::Thing::from(report_api_key)))
            .bind((description_binding, report_api_key.description.to_owned()))
            .bind((created_by_binding, surrealdb::sql::Thing::from(&report_api_key.created_by)))
    }

    fn revoke_report_api_key_query(
        self,
        report_api_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let report_api_key_binding = next_binding();
        let revoked_by_binding = next_binding();

        self.query(
            format!("UPDATE ${report_api_key_binding} SET revoked_at = time::now(), revoked_by = ${revoked_by_binding} WHERE revoked_at IS NONE"),
        )
        .bind((
            report_api_key_binding,
            surrealdb::sql::Thing::from((
                "report_api_key",
                surrealdb::sql::Id::from(report_api_key_id as i64),
            )),
        ))
        .bind((revoked_by_binding, surrealdb::sql::Thing::from(revoked_by)))
    }

    fn report_api_key_is_valid_query(
        self,
        report_api_key_id: u32,
    ) -> surrealdb::method::Query<'r, C> {
        let report_api_key_binding = next_binding();

        self.query(format!(
            "SELECT type::is::none(revoked_at) AS valid FROM ${report_api_key_binding}"
        ))
        .bind((
            report_api_key_binding,
            surrealdb::sql::Thing::from((
                "report_api_key",
                surrealdb::sql::Id::from(report_api_key_id as i64),
            )),
        ))
    }

    type ReportApiKeyIsValidQueryResponse = ReportApiKeyIsValidQueryResponse;
}

impl From<&ReportApiKey> for surrealdb::sql::Thing {
    fn from(report_api_key: &ReportApiKey) -> Self {
        Self::from((
            "report_api_key",
            surrealdb::sql::Id::Number(report_api_key.id as i64),
        ))
    }
}
