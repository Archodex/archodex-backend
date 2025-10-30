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
use tracing::instrument;

use crate::{db::DBConnection, env::Env, next_binding, surrealdb_deserializers, user::User};

#[derive(Clone, Debug, Deserialize)]
pub struct ReportApiKey {
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
    #[must_use]
    pub fn new(description: Option<String>, created_by: User) -> Self {
        Self {
            id: rand::thread_rng().gen_range::<u32, _>(100_000..=999_999),
            description,
            created_at: None,
            created_by,
            revoked_at: None,
            revoked_by: None,
        }
    }

    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Generates an encrypted API key value string.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `account_id` cannot be parsed as a valid u64
    /// - Encryption of the account ID fails
    #[instrument(err)]
    pub async fn generate_value(
        &self,
        account_id: &str,
        account_salt: Vec<u8>,
        accounts_db: Option<&DBConnection>,
    ) -> anyhow::Result<String> {
        let cipher = Aes128Gcm::new(&Env::api_private_key(accounts_db).await);
        let nonce = Aes128Gcm::generate_nonce(&mut rand::rngs::OsRng);

        let message = proto::ReportApiKeyEncryptedContents {
            account_id: account_id.parse::<u64>().context("Invalid account ID")?,
        };

        let aad = proto::ReportApiKeyEncryptedAad {
            key_id: self.id,
            #[cfg(feature = "archodex-com")]
            endpoint: Some(Env::endpoint().to_owned()),
            #[cfg(not(feature = "archodex-com"))]
            endpoint: None,
            account_salt: account_salt.clone(),
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
            version: 1,
            #[cfg(feature = "archodex-com")]
            endpoint: Some(Env::endpoint().to_owned()),
            #[cfg(not(feature = "archodex-com"))]
            endpoint: None,
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

    /// Validates a report key value and extracts the account ID and key ID.
    ///
    /// This method validates that a report key value contains the correct endpoint and returns
    /// the account and key IDs. The caller must still validate the key ID exists for the account
    /// and has not been revoked.
    ///
    /// # Errors
    ///
    /// Returns an error if the key value is invalid due to:
    /// - Format: missing prefix, invalid separator, malformed base64/protobuf
    /// - Key ID: not a number or out of range (100000-999999)
    /// - Endpoint: mismatch with current environment (archodex-com only)
    /// - Encryption: invalid salt length, decryption failure, or tampered data
    /// - Account ID: out of range (must be >= 1000000000)
    #[instrument(err, skip_all)]
    pub async fn validate_value(
        report_api_key_value: &str,
        accounts_db: Option<&DBConnection>,
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
            (100_000..=999_999).contains(&key_id),
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

        #[cfg(feature = "archodex-com")]
        {
            let Some(endpoint) = &value.endpoint else {
                bail!("Invalid report key value: Missing archodex.com endpoint");
            };

            ensure!(
                endpoint == Env::endpoint(),
                format!(
                    "Invalid report key value: Incorrect archodex.com endpoint (key: {endpoint})"
                )
            );
        }
        #[cfg(not(feature = "archodex-com"))]
        if let Some(endpoint) = value.endpoint {
            bail!("Invalid report key value: Key is meant for archodex.com endpoint {endpoint:?}");
        }

        ensure!(
            value.account_salt.len() == 16,
            "Invalid report key value: Account salt is not 16 bytes long"
        );

        let nonce = aead::Nonce::<Aes128Gcm>::from_slice(&value.nonce);
        let cipher = Aes128Gcm::new(&Env::api_private_key(accounts_db).await);

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
            encrypted_contents.account_id >= 1_000_000_000,
            "Invalid report key value: Account ID is out of range"
        );

        Ok((encrypted_contents.account_id.to_string(), key_id))
    }
}

pub(crate) trait ReportApiKeyQueries<'r, C: surrealdb::Connection> {
    fn list_report_api_keys_query(&'r self) -> surrealdb::method::Query<'r, C>;
    fn create_report_api_key_query(
        &'r self,
        report_api_key: &ReportApiKey,
    ) -> surrealdb::method::Query<'r, C>;
    fn revoke_report_api_key_query(
        &'r self,
        report_api_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn report_api_key_is_valid_query(&'r self, id: u32) -> surrealdb::method::Query<'r, C>;
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

impl<'r, C: surrealdb::Connection> ReportApiKeyQueries<'r, C> for surrealdb::Surreal<C> {
    fn list_report_api_keys_query(&'r self) -> surrealdb::method::Query<'r, C> {
        self.query("SELECT * FROM report_api_key WHERE type::is::none(revoked_at)")
    }

    fn create_report_api_key_query(
        &'r self,
        report_api_key: &ReportApiKey,
    ) -> surrealdb::method::Query<'r, C> {
        let report_api_key_binding = next_binding();
        let description_binding = next_binding();
        let created_by_binding = next_binding();

        self
            .query(format!("CREATE ${report_api_key_binding} CONTENT {{ description: ${description_binding}, created_by: ${created_by_binding} }}"))
            .bind((report_api_key_binding, surrealdb::sql::Thing::from(report_api_key)))
            .bind((description_binding, report_api_key.description.clone()))
            .bind((created_by_binding, surrealdb::sql::Thing::from(&report_api_key.created_by)))
    }

    fn revoke_report_api_key_query(
        &'r self,
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
                surrealdb::sql::Id::from(i64::from(report_api_key_id)),
            )),
        ))
        .bind((revoked_by_binding, surrealdb::sql::Thing::from(revoked_by)))
    }

    fn report_api_key_is_valid_query(
        &'r self,
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
                surrealdb::sql::Id::from(i64::from(report_api_key_id)),
            )),
        ))
    }

    type ReportApiKeyIsValidQueryResponse = ReportApiKeyIsValidQueryResponse;
}

impl From<&ReportApiKey> for surrealdb::sql::Thing {
    fn from(report_api_key: &ReportApiKey) -> Self {
        Self::from((
            "report_api_key",
            surrealdb::sql::Id::Number(i64::from(report_api_key.id)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1: Invalid prefix rejection
    #[tokio::test]
    async fn test_validate_value_invalid_prefix() {
        let result = ReportApiKey::validate_value("invalid_prefix_123456_abc", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing prefix"));
    }

    /// Test 2: Invalid format (missing underscore separator after `key_id`)
    #[tokio::test]
    async fn test_validate_value_invalid_format_missing_separator() {
        let result = ReportApiKey::validate_value("archodex_report_api_key_123456", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid format"));
    }

    /// Test 3: Key ID not a number
    #[tokio::test]
    async fn test_validate_value_key_id_not_a_number() {
        let result =
            ReportApiKey::validate_value("archodex_report_api_key_abc_dGVzdA==", None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Key ID is not a number")
        );
    }

    /// Test 4: Key ID out of range (< 100000)
    #[tokio::test]
    async fn test_validate_value_key_id_below_range() {
        let result =
            ReportApiKey::validate_value("archodex_report_api_key_99999_dGVzdA==", None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Key ID is out of range")
        );
    }

    /// Test 5: Key ID out of range (> 999999)
    #[tokio::test]
    async fn test_validate_value_key_id_above_range() {
        let result =
            ReportApiKey::validate_value("archodex_report_api_key_1000000_dGVzdA==", None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Key ID is out of range")
        );
    }

    /// Test 6: Base64 decode failure
    #[tokio::test]
    async fn test_validate_value_base64_decode_failure() {
        let result =
            ReportApiKey::validate_value("archodex_report_api_key_123456_!!!invalid!!!", None)
                .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to base64 decode")
        );
    }

    /// Test 7: Protobuf decode failure (valid base64, invalid protobuf)
    #[tokio::test]
    async fn test_validate_value_protobuf_decode_failure() {
        // "invalid_protobuf" encoded as base64
        let result = ReportApiKey::validate_value(
            "archodex_report_api_key_123456_aW52YWxpZF9wcm90b2J1Zg==",
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to decode report key value as protobuf")
        );
    }
}
