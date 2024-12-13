use std::{collections::HashMap, ops::Deref, sync::LazyLock};

use reqwest::Url;
use tokio::sync::{OnceCell, RwLock};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mode {
    LocalDev,
    RemoteDev,
    Production,
}

pub(crate) struct Env {
    mode: Mode,
    aws_partition: String,
    aws_region: String,
    backend_aws_account_id: String,
    endpoint: String,
    customer_data_ou_id: String,
    cognito_issuer_endpoint: String,
    cognito_user_pool_id: String,
    cognito_client_id: String,
    cognito_auth_endpoint: Url,
    cognito_redirect_uri: String,
    cognito_refresh_token_validity_in_days: u16,
    app_redirect_uri: Url,
}

impl Env {
    pub(crate) fn get() -> &'static Self {
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

            Env {
                mode,
                aws_partition: std::env::var("AWS_PARTITION")
                    .expect("Missing AWS_PARTITION env var"),
                aws_region: std::env::var("AWS_REGION").expect("Missing AWS_REGION env var"),
                backend_aws_account_id: std::env::var("AWS_ACCOUNT_ID")
                    .expect("Missing AWS_ACCOUNT_ID env var"),
                endpoint: std::env::var("ENDPOINT").expect("Missing ENDPOINT env var"),
                customer_data_ou_id: std::env::var("CUSTOMER_DATA_OU_ID")
                    .expect("Missing CUSTOMER_DATA_OU_ID env var"),
                cognito_issuer_endpoint: std::env::var("COGNITO_ISSUER_ENDPOINT")
                    .expect("Missing COGNITO_ISSUER_ENDPOINT env var"),
                cognito_user_pool_id: std::env::var("COGNITO_USER_POOL_ID")
                    .expect("Missing COGNITO_USER_POOL_ID env var"),
                cognito_client_id: std::env::var("COGNITO_CLIENT_ID")
                    .expect("Missing COGNITO_CLIENT_ID env var"),
                cognito_auth_endpoint: Url::parse(
                    &std::env::var("COGNITO_AUTH_ENDPOINT")
                        .expect("Missing COGNITO_AUTH_ENDPOINT env var"),
                )
                .expect("Failed to parse env var COGNITO_AUTH_ENDPOINT as a URL"),
                cognito_redirect_uri: std::env::var("COGNITO_REDIRECT_URI")
                    .expect("Missing COGNITO_REDIRECT_URI env var"),
                cognito_refresh_token_validity_in_days: std::env::var(
                    "COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS",
                )
                .expect("Missing COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS env var")
                .parse()
                .expect("Failed to parse COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS as a u16"),
                app_redirect_uri: Url::parse(
                    &std::env::var("APP_REDIRECT_URI").expect("Missing APP_REDIRECT_URI env var"),
                )
                .expect("Failed to parse env var APP_REDIRECT_URI as a URL"),
            }
        });

        ENV.deref()
    }

    pub(crate) fn is_local_dev() -> bool {
        Self::get().mode == Mode::LocalDev
    }

    pub(crate) fn aws_partition() -> &'static str {
        Self::get().aws_partition.as_str()
    }

    pub(crate) fn aws_region() -> &'static str {
        Self::get().aws_region.as_str()
    }

    pub(crate) fn backend_aws_account_id() -> &'static str {
        Self::get().backend_aws_account_id.as_str()
    }

    pub(crate) fn endpoint() -> &'static str {
        Self::get().endpoint.as_str()
    }

    pub(crate) fn customer_data_aws_account_id() -> &'static str {
        Self::get().customer_data_ou_id.as_str()
    }

    pub(crate) fn cognito_issuer_endpoint() -> &'static str {
        Self::get().cognito_issuer_endpoint.as_str()
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

    pub(crate) fn cognito_redirect_uri() -> &'static str {
        Self::get().cognito_redirect_uri.as_str()
    }

    pub(crate) fn cognito_refresh_token_validity_in_days() -> u16 {
        Self::get().cognito_refresh_token_validity_in_days
    }

    pub(crate) fn app_redirect_uri() -> &'static Url {
        &Self::get().app_redirect_uri
    }

    async fn aws_config() -> &'static aws_config::SdkConfig {
        static AWS_CONFIG: OnceCell<aws_config::SdkConfig> = OnceCell::const_new();

        AWS_CONFIG
            .get_or_init(|| async { aws_config::load_from_env().await })
            .await
    }

    pub(crate) async fn aws_organizations_client() -> &'static aws_sdk_organizations::Client {
        static CLIENT: OnceCell<aws_sdk_organizations::Client> = OnceCell::const_new();

        CLIENT
            .get_or_init(|| async { aws_sdk_organizations::Client::new(Self::aws_config().await) })
            .await
    }

    pub(crate) async fn aws_cloudwatch_client() -> &'static aws_sdk_cloudwatch::Client {
        static CLIENT: OnceCell<aws_sdk_cloudwatch::Client> = OnceCell::const_new();

        CLIENT
            .get_or_init(|| async { aws_sdk_cloudwatch::Client::new(Self::aws_config().await) })
            .await
    }

    async fn aws_dynamodb_local_client() -> &'static aws_sdk_dynamodb::Client {
        static CLIENT: OnceCell<aws_sdk_dynamodb::Client> = OnceCell::const_new();

        CLIENT
            .get_or_init(|| async {
                aws_sdk_dynamodb::Client::new(
                    &aws_config::from_env().profile_name("ddbtest").load().await,
                )
            })
            .await
    }

    pub(crate) fn aws_customer_data_account_role_arn(customer_data_aws_account_id: &str) -> String {
        format!(
            "arn:{aws_partition}:iam::{customer_data_aws_account_id}:role/BackendAPICustomerDataManagementRole",
            aws_partition = Self::aws_partition(),
        )
    }

    pub(crate) async fn aws_dynamodb_client_for_customer_data_account(
        archodex_account_id: &str,
        customer_data_aws_account_id: &str,
    ) -> aws_sdk_dynamodb::Client {
        if Self::is_local_dev() {
            return Self::aws_dynamodb_local_client().await.clone();
        }

        static CLIENTS: LazyLock<RwLock<HashMap<String, aws_sdk_dynamodb::Client>>> =
            LazyLock::new(|| RwLock::new(HashMap::new()));

        let clients_by_account_id = CLIENTS.read().await;

        if let Some(client) = clients_by_account_id.get(customer_data_aws_account_id) {
            client.clone()
        } else {
            drop(clients_by_account_id);

            let mut clients_by_account_id = CLIENTS.write().await;

            match clients_by_account_id.get(customer_data_aws_account_id) {
                Some(client) => client.clone(),
                None => {
                    let provider = aws_config::sts::AssumeRoleProvider::builder(
                        &Self::aws_customer_data_account_role_arn(customer_data_aws_account_id),
                    )
                    .session_name(format!(
                        "create-account-{archodex_account_id}-service-data-table"
                    ))
                    .build()
                    .await;

                    let config = aws_config::from_env()
                        .credentials_provider(provider)
                        .load()
                        .await;

                    let client = aws_sdk_dynamodb::Client::new(&config);

                    clients_by_account_id
                        .insert(customer_data_aws_account_id.to_string(), client.clone());

                    client
                }
            }
        }
    }

    pub(crate) async fn api_key_kms_data_key() -> &'static aes_gcm::Key<aes_gcm::Aes128Gcm> {
        use base64::prelude::*;

        static DATA_KEY: OnceCell<aes_gcm::Key<aes_gcm::Aes128Gcm>> = OnceCell::const_new();

        DATA_KEY
            .get_or_init(|| async {
                let ssm_client = aws_sdk_ssm::Client::new(Self::aws_config().await);

                let encrypted_data_key_base64 = ssm_client
                    .get_parameter()
                    .name("api_key_customer_data_key")
                    .send()
                    .await
                    .expect("Failed to get API key")
                    .parameter
                    .expect("SSM GetParameter response missing Parameter")
                    .value
                    .expect("SSM GetParameter response missing Parameter value");

                let encrypted_data_key = BASE64_STANDARD
                    .decode(encrypted_data_key_base64)
                    .expect("Failed to decode API Keys encrypted data key");

                let encrypted_data_key = aws_smithy_types::Blob::new(encrypted_data_key);

                let kms_client = aws_sdk_kms::Client::new(Self::aws_config().await);

                let data_key = kms_client
                    .decrypt()
                    .ciphertext_blob(encrypted_data_key)
                    .encryption_context("Purpose", "APIKeys")
                    .send()
                    .await
                    .expect("Failed to decrypt data key")
                    .plaintext
                    .expect("KMS Decrypt response missing Plaintext");

                let data_key = aes_gcm::Key::<aes_gcm::Aes128Gcm>::clone_from_slice(
                    data_key.into_inner().as_slice(),
                );

                data_key
            })
            .await
    }
}
