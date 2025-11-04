use std::sync::LazyLock;

#[cfg(not(feature = "archodex-com"))]
use tokio::sync::RwLock;

pub struct Env {
    port: u16,
    archodex_domain: String,
    accounts_surrealdb_url: String,
    #[cfg(not(feature = "archodex-com"))]
    surrealdb_url: String,
    surrealdb_creds: Option<surrealdb::opt::auth::Root<'static>>,
    #[cfg(feature = "archodex-com")]
    endpoint: String,
    cognito_user_pool_id: String,
    cognito_client_id: String,
    #[cfg(not(feature = "archodex-com"))]
    api_private_key: RwLock<Option<aes_gcm::Key<aes_gcm::Aes128Gcm>>>,
}

impl Env {
    fn get() -> &'static Self {
        static ENV: LazyLock<Env> = LazyLock::new(|| {
            let port = std::env::var("PORT")
                .unwrap_or_else(|_| {
                    #[cfg(not(feature = "archodex-com"))]
                    {
                        "5732".into()
                    }

                    #[cfg(feature = "archodex-com")]
                    {
                        "5731".into()
                    }
                })
                .parse::<u16>()
                .expect("Failed to parse PORT env var as u16");

            let archodex_domain = env_with_default_for_empty("ARCHODEX_DOMAIN", "archodex.com");

            #[cfg(not(feature = "archodex-com"))]
            let (_, surrealdb_url) = (
                std::env::var("ACCOUNTS_SURREALDB_URL").expect_err(
                    "ACCOUNTS_SURREALDB_URL env var should not be set in non-archodex-com builds",
                ),
                env_with_default_for_empty("SURREALDB_URL", "rocksdb://db"),
            );

            #[cfg(feature = "archodex-com")]
            let (accounts_surrealdb_url, _) = (
                std::env::var("ACCOUNTS_SURREALDB_URL")
                    .expect("Missing ACCOUNTS_SURREALDB_URL env var"),
                std::env::var("SURREALDB_URL")
                    .expect_err("SURREALDB_URL env var should not be set in archodex-com builds"),
            );

            let surrealdb_username = match std::env::var("SURREALDB_USERNAME") {
                Ok(surrealdb_username) if !surrealdb_username.is_empty() => {
                    Some(surrealdb_username)
                }
                Ok(_) | Err(std::env::VarError::NotPresent) => None,
                Err(err) => panic!("Invalid SURREALDB_USERNAME env var: {err:?}"),
            };
            let surrealdb_password = match std::env::var("SURREALDB_PASSWORD") {
                Ok(surrealdb_password) if !surrealdb_password.is_empty() => {
                    Some(surrealdb_password)
                }
                Ok(_) | Err(std::env::VarError::NotPresent) => None,
                Err(err) => panic!("Invalid SURREALDB_PASSWORD env var: {err:?}"),
            };

            let surrealdb_creds = match (surrealdb_username, surrealdb_password) {
                (Some(surrealdb_username), Some(surrealdb_password)) => {
                    Some(surrealdb::opt::auth::Root {
                        username: Box::leak(Box::new(surrealdb_username)),
                        password: Box::leak(Box::new(surrealdb_password)),
                    })
                }
                (None, None) => None,
                _ => panic!(
                    "Both SURREALDB_USERNAME and SURREALDB_PASSWORD must be set or unset together"
                ),
            };

            Env {
                port,
                archodex_domain,
                #[cfg(feature = "archodex-com")]
                accounts_surrealdb_url,
                #[cfg(not(feature = "archodex-com"))]
                accounts_surrealdb_url: surrealdb_url.to_string(),
                #[cfg(not(feature = "archodex-com"))]
                surrealdb_url,
                surrealdb_creds,
                #[cfg(feature = "archodex-com")]
                endpoint: std::env::var("ENDPOINT").expect("Missing ENDPOINT env var"),
                cognito_user_pool_id: env_with_default_for_empty(
                    "COGNITO_USER_POOL_ID",
                    "us-west-2_Mf1K95El6",
                ),
                cognito_client_id: env_with_default_for_empty(
                    "COGNITO_CLIENT_ID",
                    "1a5vsre47o6pa39p3p81igfken",
                ),
                #[cfg(not(feature = "archodex-com"))]
                api_private_key: RwLock::new(None),
            }
        });

        &ENV
    }

    #[must_use]
    pub fn port() -> u16 {
        Self::get().port
    }

    #[must_use]
    pub fn archodex_domain() -> &'static str {
        Self::get().archodex_domain.as_str()
    }

    #[must_use]
    pub fn accounts_surrealdb_url() -> &'static str {
        Self::get().accounts_surrealdb_url.as_str()
    }

    #[cfg(not(feature = "archodex-com"))]
    pub(crate) fn surrealdb_url() -> &'static str {
        Self::get().surrealdb_url.as_str()
    }

    #[must_use]
    pub fn surrealdb_creds() -> Option<surrealdb::opt::auth::Root<'static>> {
        Self::get().surrealdb_creds
    }

    #[cfg(feature = "archodex-com")]
    pub(crate) fn endpoint() -> &'static str {
        Self::get().endpoint.as_str()
    }

    pub(crate) fn cognito_user_pool_id() -> &'static str {
        Self::get().cognito_user_pool_id.as_str()
    }

    pub(crate) fn cognito_client_id() -> &'static str {
        Self::get().cognito_client_id.as_str()
    }

    pub(crate) async fn api_private_key(
        accounts_db_conn: Option<&crate::db::DBConnection>,
    ) -> aes_gcm::Key<aes_gcm::Aes128Gcm> {
        // In self-hosted mode we use either the API private key material from the ARCHODEX_API_PRIVATE_KEY environment
        // variable or from the account database record. If neither exists we panic. If both exist we also panic, as
        // this is almost certainly a misconfiguration.
        //
        // The purpose of the ARCHODEX_API_PRIVATE_KEY is to allow the key material to be stored elsewhere outside of
        // the database, but if it isn't set then we generate key material when the account is created and save it in
        // the database.
        #[cfg(not(feature = "archodex-com"))]
        {
            use serde::Deserialize;

            use crate::{db::QueryCheckFirstRealError as _, surrealdb_deserializers};

            #[derive(Deserialize)]
            struct ApiPrivateKeyResult {
                #[serde(
                    default,
                    deserialize_with = "surrealdb_deserializers::bytes::deserialize_optional"
                )]
                api_private_key: Option<Vec<u8>>,
            }

            if let Some(api_private_key) = Self::get().api_private_key.read().await.as_ref() {
                return *api_private_key;
            }

            let mut lock = Self::get().api_private_key.write().await;
            if let Some(api_private_key) = lock.as_ref() {
                return *api_private_key;
            }

            let api_private_key_from_db = {
                let db_conn = accounts_db_conn
                    .expect("accounts_db_conn is required in non-archodex-com builds");

                match db_conn {
                    crate::db::DBConnection::Concurrent(db) => {
                        db.query(
                            "SELECT api_private_key FROM account WHERE deleted_at IS NONE LIMIT 1",
                        )
                        .await
                        .expect("should be able to query accounts database")
                        .check_first_real_error()
                        .expect("should be able to check query errors")
                        .take::<Option<ApiPrivateKeyResult>>(0)
                        .expect("should be able to take first result")
                        .expect("should be able to extract api_private_key from result")
                        .api_private_key
                    }
                    #[cfg(feature = "rocksdb")]
                    crate::db::DBConnection::Nonconcurrent(db) => {
                        db.query(
                            "SELECT api_private_key FROM account WHERE deleted_at IS NONE LIMIT 1",
                        )
                        .await
                        .expect("should be able to query accounts database")
                        .check_first_real_error()
                        .expect("should be able to check query errors")
                        .take::<Option<ApiPrivateKeyResult>>(0)
                        .expect("should be able to take first result")
                        .expect("should be able to extract api_private_key from result")
                        .api_private_key
                    }
                }
            };

            let api_private_key_from_env = match std::env::var("ARCHODEX_API_PRIVATE_KEY") {
                Ok(hex_bytes) => {
                    let bytes = hex::decode(hex_bytes).expect(
                        "environment variable ARCHODEX_API_PRIVATE_KEY must be hex encoded",
                    );

                    assert!(
                        bytes.len() == 16,
                        "environment variable ARCHODEX_API_PRIVATE_KEY must be 16 bytes hex encoded"
                    );

                    Some(bytes)
                }
                Err(_) => None,
            };

            let api_private_key_bytes = match (api_private_key_from_db, api_private_key_from_env) {
                (Some(_), Some(_)) => panic!(
                    "ARCHODEX_API_PRIVATE_KEY environment variable must not be set if the variable was not set when this account was created"
                ),
                (Some(db_bytes), None) => db_bytes,
                (None, Some(env_bytes)) => env_bytes,
                (None, None) => panic!(
                    "Missing ARCHODEX_API_PRIVATE_KEY environment variable, it must be set to the same value as when this account was created"
                ),
            };

            let api_private_key =
                aes_gcm::Key::<aes_gcm::Aes128Gcm>::clone_from_slice(&api_private_key_bytes);

            lock.replace(api_private_key);
            api_private_key
        }

        #[cfg(feature = "archodex-com")]
        {
            archodex_com::api_private_key().await.clone()
        }
    }

    #[cfg(not(feature = "archodex-com"))]
    pub async fn clear_api_private_key() {
        // Only clear generated private keys, which is the case when the ARCHODEX_API_PRIVATE_KEY env var is not set
        if std::env::var("ARCHODEX_API_PRIVATE_KEY").is_err() {
            Self::get().api_private_key.write().await.take();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn force_clear_api_private_key() {
        Self::get().api_private_key.write().await.take();
    }

    #[cfg(feature = "archodex-com")]
    pub(crate) fn user_account_limit() -> u32 {
        5
    }
}

fn env_with_default_for_empty(var: &str, default: &str) -> String {
    match std::env::var(var) {
        Err(std::env::VarError::NotPresent) => default.to_string(),
        Ok(value) if value.is_empty() => default.to_string(),
        Ok(value) => value,
        Err(err) => panic!("Invalid {var} env var: {err:?}"),
    }
}
