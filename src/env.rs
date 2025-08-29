use std::{ops::Deref, sync::LazyLock};

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
    archodex_domain: String,
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

            let archodex_domain = env_with_default_for_empty("ARCHODEX_DOMAIN", "archodex.com");

            let endpoint = std::env::var("ENDPOINT").expect("Missing ENDPOINT env var");

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

            let app_redirect_uri = match std::env::var("LOCAL_FRONTEND") {
                Ok(local_frontend) if local_frontend.to_lowercase() == "true" => {
                    Url::parse("http://localhost:5173/oauth2/idpresponse")
                        .expect("Failed to parse local frontend redirect URL")
                }
                Ok(_) | Err(std::env::VarError::NotPresent) => {
                    Url::parse(&format!("https://app.{archodex_domain}/oauth2/idpresponse"))
                        .expect("Failed to parse default app redirect URL")
                }
                Err(err) => panic!("Invalid LOCAL_FRONTEND env var: {err:?}"),
            };

            Env {
                mode,
                port,
                archodex_domain: archodex_domain.clone(),
                #[cfg(feature = "archodex-com")]
                accounts_surrealdb_url,
                #[cfg(not(feature = "archodex-com"))]
                accounts_surrealdb_url: surrealdb_url.to_string(),
                #[cfg(not(feature = "archodex-com"))]
                surrealdb_url,
                surrealdb_creds,
                endpoint: endpoint.clone(),
                cognito_user_pool_id: env_with_default_for_empty(
                    "COGNITO_USER_POOL_ID",
                    "us-west-2_Mf1K95El6",
                ),
                cognito_client_id: env_with_default_for_empty(
                    "COGNITO_CLIENT_ID",
                    "1a5vsre47o6pa39p3p81igfken",
                ),
                cognito_auth_endpoint: Url::parse(&format!("https://auth.{archodex_domain}"))
                    .expect("Failed to parse auth endpoint as a URL"),
                cognito_redirect_uri: env_with_default_for_empty(
                    "COGNITO_REDIRECT_URI",
                    &format!("{endpoint}/oauth2/idpresponse"),
                ),
                cognito_refresh_token_validity_in_days: std::env::var(
                    "COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS",
                )
                .unwrap_or_else(|_| "1".to_string())
                .parse()
                .expect("Failed to parse COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS as a u16"),
                app_redirect_uri,
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

    pub fn archodex_domain() -> &'static str {
        Self::get().archodex_domain.as_str()
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

fn env_with_default_for_empty(var: &str, default: &str) -> String {
    match std::env::var(var) {
        Err(std::env::VarError::NotPresent) => default.to_string(),
        Ok(value) if value.is_empty() => default.to_string(),
        Ok(value) => value,
        Err(err) => panic!("Invalid {var} env var: {err:?}"),
    }
}
