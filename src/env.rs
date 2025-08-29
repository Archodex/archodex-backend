use std::{collections::HashSet, ops::Deref, sync::LazyLock};

use reqwest::Url;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mode {
    LocalDev,
    RemoteDev,
    Production,
}

pub struct Env {
    mode: Mode,
    port: u16,
    accounts_surrealdb_url: String,
    #[cfg(not(feature = "archodex-com"))]
    surrealdb_url: String,
    surrealdb_creds: Option<surrealdb::opt::auth::Root<'static>>,
    endpoint: String,
    cognito_user_pool_id: String,
    cognito_client_id: String,
    cognito_auth_endpoint: Url,
    cognito_redirect_uri: String,
    cognito_refresh_token_validity_in_days: u16,
    app_redirect_uri: Url,
    cors_allowed_origin_suffixes: HashSet<String>,
}

impl Env {
    fn get() -> &'static Self {
        static ENV: LazyLock<Env> = LazyLock::new(|| {
            let mode = match std::env::var("ARCHODEX_DEV_MODE") {
                Ok(dev_mode) => match dev_mode.as_str() {
                    "local" => Mode::LocalDev,
                    "remote" => Mode::RemoteDev,
                    mode => panic!("Invalid ARCHODEX_DEV_MODE {mode:?}"),
                },
                Err(std::env::VarError::NotPresent) => Mode::Production,
                Err(err) => panic!("Invalid ARCHODEX_DEV_MODE {err:?}"),
            };

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

            #[cfg(not(feature = "archodex-com"))]
            let (_, surrealdb_url) = (
                std::env::var("ACCOUNTS_SURREALDB_URL").expect_err(
                    "ACCOUNTS_SURREALDB_URL env var should not be set in non-archodex-com builds",
                ),
                std::env::var("SURREALDB_URL").expect("Missing SURREALDB_URL env var"),
            );

            #[cfg(feature = "archodex-com")]
            let (accounts_surrealdb_url, _) = (
                std::env::var("ACCOUNTS_SURREALDB_URL")
                    .expect("Missing ACCOUNTS_SURREALDB_URL env var"),
                std::env::var("SURREALDB_URL")
                    .expect_err("SURREALDB_URL env var should not be set in archodex-com builds"),
            );

            let surrealdb_username = std::env::var("SURREALDB_USERNAME").ok();
            let surrealdb_password = std::env::var("SURREALDB_PASSWORD").ok();

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

            let endpoint = std::env::var("ENDPOINT").expect("Missing ENDPOINT env var");

            let cognito_auth_endpoint = std::env::var("COGNITO_AUTH_ENDPOINT")
                .unwrap_or_else(|_| "https://auth.archodex.com".to_string());

            let mut cors_allowed_origin_suffixes = HashSet::from([
                "https://app.archodex.com".to_string(),
                "http://localhost:5173".to_string(),
            ]);

            if cognito_auth_endpoint != "https://auth.archodex.com" {
                let url = Url::parse(&cognito_auth_endpoint)
                    .expect("Failed to parse COGNITO_AUTH_ENDPOINT as a URL");

                let second_level_domain = url
                    .host_str()
                    .and_then(|host| {
                        let parts: Vec<&str> = host.rsplitn(2, '.').collect();
                        if parts.len() >= 2 {
                            Some(format!("{}.{}", parts[1], parts[0]))
                        } else {
                            None
                        }
                    })
                    .expect("Failed to extract second-level domain from COGNITO_AUTH_ENDPOINT");

                cors_allowed_origin_suffixes.insert(second_level_domain);
            }

            Env {
                mode,
                port,
                #[cfg(feature = "archodex-com")]
                accounts_surrealdb_url,
                #[cfg(not(feature = "archodex-com"))]
                accounts_surrealdb_url: surrealdb_url.to_string(),
                #[cfg(not(feature = "archodex-com"))]
                surrealdb_url,
                surrealdb_creds,
                endpoint: endpoint.clone(),
                cognito_user_pool_id: std::env::var("COGNITO_USER_POOL_ID")
                    .unwrap_or_else(|_| "us-west-2_Mf1K95El6".to_string()),
                cognito_client_id: std::env::var("COGNITO_CLIENT_ID")
                    .unwrap_or_else(|_| "1a5vsre47o6pa39p3p81igfken".to_string()),
                cognito_auth_endpoint: Url::parse(
                    &std::env::var("COGNITO_AUTH_ENDPOINT")
                        .unwrap_or_else(|_| "https://auth.archodex.com".to_string()),
                )
                .expect("Failed to parse env var COGNITO_AUTH_ENDPOINT as a URL"),
                cognito_redirect_uri: std::env::var("COGNITO_REDIRECT_URI")
                    .unwrap_or_else(|_| format!("{endpoint}/oauth2/idpresponse")),
                cognito_refresh_token_validity_in_days: std::env::var(
                    "COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS",
                )
                .unwrap_or_else(|_| "1".to_string())
                .parse()
                .expect("Failed to parse COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS as a u16"),
                app_redirect_uri: Url::parse(
                    &std::env::var("APP_REDIRECT_URI").unwrap_or_else(|_| {
                        "https://app.archodex.com/oauth2/idpresponse".to_string()
                    }),
                )
                .expect("Failed to parse env var APP_REDIRECT_URI as a URL"),
                cors_allowed_origin_suffixes,
            }
        });

        ENV.deref()
    }

    pub(crate) fn is_local_dev() -> bool {
        Self::get().mode == Mode::LocalDev
    }

    pub fn port() -> u16 {
        Self::get().port
    }

    pub fn accounts_surrealdb_url() -> &'static str {
        Self::get().accounts_surrealdb_url.as_str()
    }

    #[cfg(not(feature = "archodex-com"))]
    pub(crate) fn surrealdb_url() -> &'static str {
        Self::get().surrealdb_url.as_str()
    }

    pub fn surrealdb_creds() -> Option<surrealdb::opt::auth::Root<'static>> {
        Self::get().surrealdb_creds
    }

    pub(crate) fn endpoint() -> &'static str {
        Self::get().endpoint.as_str()
    }

    pub(crate) fn cognito_user_pool_id() -> &'static str {
        Self::get().cognito_user_pool_id.as_str()
    }

    pub(crate) fn cognito_client_id() -> &'static str {
        Self::get().cognito_client_id.as_str()
    }

    pub(crate) fn cognito_auth_endpoint() -> &'static Url {
        &Self::get().cognito_auth_endpoint
    }

    pub(crate) fn cognito_redirect_uri(is_local_dev: bool) -> &'static str {
        if is_local_dev {
            static LOCAL_DEV_REDIRECT_URI: LazyLock<String> =
                LazyLock::new(|| format!("{}/local", Env::get().cognito_redirect_uri));

            &LOCAL_DEV_REDIRECT_URI
        } else {
            Self::get().cognito_redirect_uri.as_str()
        }
    }

    pub(crate) fn cognito_refresh_token_validity_in_days() -> u16 {
        Self::get().cognito_refresh_token_validity_in_days
    }

    pub(crate) fn app_redirect_uri(is_local_dev: bool) -> &'static Url {
        if is_local_dev {
            static LOCAL_DEV_URL: LazyLock<Url> = LazyLock::new(|| {
                Url::parse("http://localhost:5173/oauth2/idpresponse")
                    .expect("Invalid local development URL")
            });

            &LOCAL_DEV_URL
        } else {
            &Self::get().app_redirect_uri
        }
    }

    pub(crate) fn cors_allowed_origin_suffixes() -> &'static HashSet<String> {
        &Self::get().cors_allowed_origin_suffixes
    }

    pub(crate) async fn api_private_key() -> &'static aes_gcm::Key<aes_gcm::Aes128Gcm> {
        #[cfg(not(feature = "archodex-com"))]
        {
            use tracing::warn;

            static API_PRIVATE_KEY: LazyLock<aes_gcm::Key<aes_gcm::Aes128Gcm>> =
                LazyLock::new(|| {
                    warn!("Using static API private key while functionality is being developed!");

                    aes_gcm::Key::<aes_gcm::Aes128Gcm>::clone_from_slice(b"archodex-api-key")
                });
            &API_PRIVATE_KEY
        }

        #[cfg(feature = "archodex-com")]
        {
            archodex_com::api_private_key().await
        }
    }
}
