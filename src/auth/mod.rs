pub mod provider;

pub use provider::{AuthProvider, RealAuthProvider};

#[cfg(any(test, feature = "test-support"))]
pub use provider::{AuthContext, FixedAuthProvider};

use std::{collections::HashMap, time::SystemTime};

use axum::{extract::Request, middleware::Next, response::Response};
use josekit::{
    JoseError,
    jwk::JwkSet,
    jws::alg::rsassa::{RsassaJwsAlgorithm, RsassaJwsVerifier},
    jwt,
};
use reqwest::header::AUTHORIZATION;
use surrealdb::{Surreal, Uuid, engine::any::Any};
use tokio::sync::OnceCell;
use tracing::{Instrument as _, error_span, info, instrument, warn};

use crate::{
    Result,
    db::{QueryCheckFirstRealError, accounts_db},
    env::Env,
    report_api_key::{ReportApiKeyIsValidQueryResponse, ReportApiKeyQueries},
    user::User,
};
use archodex_error::{
    anyhow::{Context as _, anyhow},
    not_found, unauthorized,
};

static JWK_SET: OnceCell<(JwkSet, HashMap<String, RsassaJwsVerifier>)> = OnceCell::const_new();

pub(crate) async fn jwks(
    jwks_issuer: &str,
) -> &'static (JwkSet, HashMap<String, RsassaJwsVerifier>) {
    JWK_SET
        .get_or_init(|| async {
            let jwks_url = format!("{jwks_issuer}/.well-known/jwks.json");

            info!("Fetching JWKS from {jwks_url}");

            let client = reqwest::Client::new();

            let jwks_bytes = client
                .get(jwks_url)
                .send()
                .await
                .expect("Failed to request Cognito jwks")
                .bytes()
                .await
                .expect("Failed to receive Cognito jwks bytes");

            let jwks =
                JwkSet::from_bytes(jwks_bytes.as_ref()).expect("Failed to parse Cognito jwks");

            let verifiers = jwks
                .keys()
                .iter()
                .map(|jwk| {
                    (
                        jwk.key_id()
                            .expect("Cognito jwk missing 'kid' field")
                            .to_owned(),
                        match jwk.algorithm() {
                            Some("RS256") => RsassaJwsAlgorithm::Rs256,
                            Some("RS384") => RsassaJwsAlgorithm::Rs384,
                            Some("RS512") => RsassaJwsAlgorithm::Rs512,
                            Some(alg) => {
                                panic!("Unsupported Cognito jwk algorithm {alg}");
                            }
                            None => {
                                panic!("Cognito jwk missing 'alg' field");
                            }
                        }
                        .verifier_from_jwk(jwk)
                        .expect("Failed to create verifier from Cognito jwk"),
                    )
                })
                .collect::<HashMap<_, _>>();

            (jwks, verifiers)
        })
        .await
}

#[derive(Clone, Debug)]
pub(crate) struct DashboardAuth {
    principal: User,
}

impl DashboardAuth {
    pub(crate) async fn authenticate(mut req: Request, next: Next) -> Result<Response> {
        let authorization = req.headers().get(AUTHORIZATION);
        let dashboard_auth = async move {
            let Some(authorization) = authorization else {
                warn!("Missing Authorization header");
                unauthorized!();
            };

            let Ok(authorization) = authorization.to_str() else {
                warn!("Failed to parse Authorization header as string");
                unauthorized!();
            };

            let Some(access_token) = authorization.strip_prefix("Bearer ") else {
                warn!("Invalid Authorization header format");
                unauthorized!();
            };

            let cognito_user_pool_id = Env::cognito_user_pool_id();
            let cognito_client_id = Env::cognito_client_id();

            let jwks_issuer =
                format!("https://cognito-idp.us-west-2.amazonaws.com/{cognito_user_pool_id}");

            let (jwk_set, verifier_map) = jwks(&jwks_issuer).await;

            let user_id = match jwt::decode_with_verifier_in_jwk_set(access_token, jwk_set, |jwk| {
                Ok(verifier_map
                    .get(jwk.key_id().ok_or(JoseError::InvalidJwkFormat(anyhow!(
                        "Cognito jwk missing 'kid' field"
                    )))?)
                    .map(|verifier| verifier as &dyn josekit::jws::JwsVerifier))
            }) {
                Ok((payload, _header)) => {
                    let Some(josekit::Value::String(sub)) = payload.claim("sub") else {
                        warn!("Missing or invalid sub claim in JWT");
                        unauthorized!();
                    };

                    let mut validator = jwt::JwtPayloadValidator::new();

                    validator.set_base_time(SystemTime::now());
                    validator.set_issuer(&jwks_issuer);
                    validator.set_claim("client_id", cognito_client_id.into());
                    validator.set_claim("token_use", "access".into());

                    match validator.validate(&payload) {
                        Ok(()) => Result::Ok(sub.to_owned()),
                        Err(err) => {
                            warn!(?err, "Failed to validate JWT");
                            unauthorized!();
                        }
                    }
                }
                Err(err) => {
                    warn!(?err, "Failed to verify JWT");
                    unauthorized!();
                }
            }?;

            let user_id = Uuid::parse_str(&user_id)
                .with_context(|| format!("Failed to parse user ID {user_id:?} as UUID"))?;

            Result::Ok(DashboardAuth {
                principal: User::new(user_id),
            })
        }
        .instrument(error_span!("authenticate"))
        .await?;

        tracing::Span::current().record("auth", tracing::field::debug(&dashboard_auth));

        req.extensions_mut().insert(dashboard_auth);

        Ok(next.run(req).await)
    }

    pub(crate) fn principal(&self) -> &User {
        &self.principal
    }

    #[instrument]
    pub(crate) async fn validate_account_access(&self, account_id: &str) -> Result<()> {
        if accounts_db()
            .await?
            .query("SELECT 1 FROM $user->has_access->(account WHERE record::id(id) == $account_id)")
            .bind(("user", surrealdb::sql::Thing::from(&self.principal)))
            .bind(("account_id", account_id.to_string()))
            .await?
            .check_first_real_error()?
            .take::<Option<u8>>((0, "1"))?
            .is_none()
        {
            warn!("Account does not exist or principal does not have access to account");
            not_found!("Account not found");
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReportApiKeyAuth {
    account_id: String,
    key_id: u32,
}

impl ReportApiKeyAuth {
    /// Creates auth context from validated credentials.
    ///
    /// Only use after authentication validation (e.g., via `AuthProvider`).
    pub(crate) fn from_credentials(account_id: String, key_id: u32) -> Self {
        Self { account_id, key_id }
    }

    pub(crate) async fn validate_account_access(&self, db: &Surreal<Any>) -> Result<()> {
        let Some(response) = db
            .report_api_key_is_valid_query(self.key_id)
            .await?
            .check_first_real_error()?
            .take::<Option<ReportApiKeyIsValidQueryResponse>>(0)?
        else {
            warn!(
                key_id = self.key_id,
                account_id = self.account_id,
                "Report key does not exist in account database",
            );
            unauthorized!();
        };

        if !response.is_valid() {
            warn!(
                key_id = self.key_id,
                account_id = self.account_id,
                "Report key was revoked in account database",
            );
            unauthorized!();
        }

        Ok(())
    }
}
