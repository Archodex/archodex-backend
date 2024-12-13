use std::{collections::HashMap, time::SystemTime};

use anyhow::{anyhow, Context};
use axum::{
    extract::{Path, Request},
    middleware::Next,
    response::Response,
};
use axum_extra::extract::CookieJar;
use josekit::{
    jwk::JwkSet,
    jws::alg::rsassa::{RsassaJwsAlgorithm, RsassaJwsVerifier},
    jwt, JoseError,
};
use surrealdb::Uuid;
use tokio::sync::OnceCell;
use tracing::{debug, info};

use crate::{env::Env, macros::*, user::User, Result};

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

#[derive(Clone)]
pub(crate) struct Auth {
    principal: User,
    account_id: Option<String>,
}

impl Auth {
    pub(crate) fn principal(&self) -> &User {
        &self.principal
    }

    pub(crate) fn account_id(&self) -> Option<&String> {
        self.account_id.as_ref()
    }
}

pub(crate) async fn auth(
    Path(params): Path<HashMap<String, String>>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let cookies = CookieJar::from_headers(req.headers());

    let Some(access_token) = cookies.get("accessToken") else {
        info!("Missing accessToken cookie");
        unauthorized!();
    };

    let cognito_issuer_endpoint = Env::cognito_issuer_endpoint();
    let cognito_user_pool_id = Env::cognito_user_pool_id();
    let cognito_client_id = Env::cognito_client_id();

    let jwks_issuer = format!("{cognito_issuer_endpoint}/{cognito_user_pool_id}");

    let (jwk_set, verifier_map) = jwks(&jwks_issuer).await;

    let user_id = match jwt::decode_with_verifier_in_jwk_set(access_token.value(), jwk_set, |jwk| {
        Ok(verifier_map
            .get(jwk.key_id().ok_or(JoseError::InvalidJwkFormat(anyhow!(
                "Cognito jwk missing 'kid' field"
            )))?)
            .map(|verifier| verifier as &dyn josekit::jws::JwsVerifier))
    }) {
        Ok((payload, _header)) => {
            let Some(josekit::Value::String(sub)) = payload.claim("sub") else {
                info!("Missing or invalid sub claim in JWT");
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
                    info!("Failed to validate JWT: {err}");
                    unauthorized!();
                }
            }
        }
        Err(err) => {
            info!("Failed to verify JWT: {err}");
            unauthorized!();
        }
    }?;

    let user_id = Uuid::parse_str(&user_id)
        .with_context(|| format!("Failed to parse user ID {user_id:?} as UUID"))?;

    debug!("Authenticated as user ID {user_id}");

    let user_id = if Env::is_local_dev() {
        let user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
            .expect("Failed to parse local development user ID");
        info!("Local development mode: Overriding user ID to {user_id}");
        user_id
    } else {
        user_id
    };

    let account_id = params.get("account_id").cloned();

    req.extensions_mut().insert(Auth {
        principal: User::new(user_id),
        account_id,
    });

    Ok(next.run(req).await)
}
