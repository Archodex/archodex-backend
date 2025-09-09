use std::{collections::HashMap, time::SystemTime};

use axum::{
    extract::{Path, Request},
    middleware::Next,
    response::Response,
};
use josekit::{
    JoseError,
    jwk::JwkSet,
    jws::alg::rsassa::{RsassaJwsAlgorithm, RsassaJwsVerifier},
    jwt,
};
use reqwest::header::AUTHORIZATION;
use surrealdb::{Surreal, Uuid, engine::any::Any, sql::statements::CommitStatement};
use tokio::sync::OnceCell;
use tracing::{info, warn};

use crate::{
    Result,
    db::{BeginReadonlyStatement, QueryCheckFirstRealError, accounts_db},
    env::Env,
    report_api_key::{ReportApiKey, ReportApiKeyIsValidQueryResponse, ReportApiKeyQueries},
    user::User,
};
use archodex_error::{
    anyhow::{Context as _, anyhow},
    unauthorized,
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

pub(crate) trait AccountAuth {
    fn account_id(&self) -> Option<&String>;
    async fn validate(&self, db: &Surreal<Any>) -> Result<()>;
}

#[derive(Clone)]
pub(crate) struct DashboardAuth {
    principal: User,
    account_id: Option<String>,
}

impl DashboardAuth {
    pub(crate) fn principal(&self) -> &User {
        &self.principal
    }
}

impl AccountAuth for DashboardAuth {
    fn account_id(&self) -> Option<&String> {
        self.account_id.as_ref()
    }

    async fn validate(&self, _db: &Surreal<Any>) -> Result<()> {
        let Some(account_id) = &self.account_id else {
            return Ok(());
        };

        if accounts_db()
            .await?
            .query(BeginReadonlyStatement)
            .query("SELECT 1 FROM $user->has_access->(account WHERE record::id(id) == $account_id)")
            .bind(("user", surrealdb::sql::Thing::from(&self.principal)))
            .bind(("account_id", account_id.to_string()))
            .query(CommitStatement::default())
            .await?
            .check_first_real_error()?
            .take::<Option<u8>>((0, "1"))?
            .is_none()
        {
            warn!(
                principal = ?self.principal,
                account_id = account_id,
                "Principal does not have access to account"
            );
            unauthorized!();
        }

        Ok(())
    }
}

pub(crate) async fn dashboard_auth(
    Path(params): Path<HashMap<String, String>>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let Some(authorization) = req.headers().get(AUTHORIZATION) else {
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

    let jwks_issuer = format!("https://cognito-idp.us-west-2.amazonaws.com/{cognito_user_pool_id}");

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
                    warn!("Failed to validate JWT: {err}");
                    unauthorized!();
                }
            }
        }
        Err(err) => {
            warn!("Failed to verify JWT: {err}");
            unauthorized!();
        }
    }?;

    let user_id = Uuid::parse_str(&user_id)
        .with_context(|| format!("Failed to parse user ID {user_id:?} as UUID"))?;

    info!("Authenticated as user ID {user_id}");

    let account_id = params.get("account_id").cloned();

    req.extensions_mut().insert(DashboardAuth {
        principal: User::new(user_id),
        account_id,
    });

    Ok(next.run(req).await)
}

#[derive(Clone)]
pub(crate) struct ReportApiKeyAuth {
    account_id: String,
    key_id: u32,
}

impl AccountAuth for ReportApiKeyAuth {
    fn account_id(&self) -> Option<&String> {
        Some(&self.account_id)
    }

    async fn validate(&self, db: &Surreal<Any>) -> Result<()> {
        let Some(response) = db
            .query(BeginReadonlyStatement)
            .report_api_key_is_valid_query(self.key_id)
            .query(CommitStatement::default())
            .await?
            .check_first_real_error()?
            .take::<Option<ReportApiKeyIsValidQueryResponse>>(0)?
        else {
            warn!(
                "Report key {key_id} does not exist in account {account_id:?}",
                key_id = self.key_id,
                account_id = self.account_id
            );
            unauthorized!();
        };

        if !response.is_valid() {
            warn!(
                "Report key {key_id} was revoked in account {account_id:?}",
                key_id = self.key_id,
                account_id = self.account_id
            );
            unauthorized!();
        }

        Ok(())
    }
}

pub(crate) async fn report_api_key_auth(mut req: Request, next: Next) -> Result<Response> {
    let Some(report_api_key_value) = req.headers().get(AUTHORIZATION) else {
        warn!("Missing Authorization header");
        unauthorized!();
    };

    let Ok(report_api_key_value) = report_api_key_value.to_str() else {
        warn!("Failed to parse Authorization header value as string");
        unauthorized!();
    };

    let (account_id, key_id) = match ReportApiKey::validate_value(report_api_key_value).await {
        Ok((account_id, key_id)) => (account_id, key_id),
        Err(err) => {
            warn!("Failed to validate report key value: {err:#?}");
            unauthorized!();
        }
    };

    info!("Validated report key value for account ID {account_id:?} and key ID {key_id}");

    req.extensions_mut().insert(ReportApiKeyAuth {
        account_id: account_id.clone(),
        key_id,
    });

    Ok(next.run(req).await)
}
