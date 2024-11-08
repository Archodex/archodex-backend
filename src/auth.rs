use std::{collections::HashMap, time::SystemTime};

use anyhow::{anyhow, Context};
use axum::{extract::Request, middleware::Next, response::Response};
use axum_extra::extract::CookieJar;
use josekit::{
    jwk::JwkSet,
    jws::alg::rsassa::{RsassaJwsAlgorithm, RsassaJwsVerifier},
    jwt, JoseError,
};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

use crate::{macros::*, Result};

#[derive(Clone)]
pub(crate) struct Principal {
    account_id: Option<u64>,
}

impl Principal {
    pub(crate) fn account_id(&self) -> Option<u64> {
        self.account_id.clone()
    }
}

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

pub(crate) async fn auth(mut req: Request, next: Next) -> Result<Response> {
    let cookies = CookieJar::from_headers(req.headers());

    let Some(access_token) = cookies.get("accessToken") else {
        info!("Missing accessToken cookie");
        unauthorized!();
    };

    let jwks_issuer_endpoint =
        std::env::var("COGNITO_ISSUER_ENDPOINT").expect("Missing COGNITO_ISSUER_ENDPOINT env var");
    let cognito_user_pool_id =
        std::env::var("COGNITO_USER_POOL_ID").expect("Missing COGNITO_USER_POOL_ID env var");
    let cognito_client_id =
        std::env::var("COGNITO_CLIENT_ID").expect("Missing COGNITO_CLIENT_ID env var");
    let endpoint = std::env::var("ENDPOINT").expect("Missing ENDPOINT env var");

    let jwks_issuer = format!("{jwks_issuer_endpoint}/{cognito_user_pool_id}");

    let (jwk_set, verifier_map) = jwks(&jwks_issuer).await;

    let sub = match jwt::decode_with_verifier_in_jwk_set(access_token.value(), jwk_set, |jwk| {
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

    debug!("Authenticated as {sub}");

    let config = if let Ok(cognito_aws_profile) = std::env::var("COGNITO_AWS_PROFILE") {
        aws_config::from_env()
            .profile_name(cognito_aws_profile)
            .load()
            .await
    } else {
        aws_config::load_from_env().await
    };

    let client = aws_sdk_cognitoidentityprovider::Client::new(&config);

    let user = client
        .admin_get_user()
        .user_pool_id(cognito_user_pool_id)
        .username(sub.clone())
        .send()
        .await
        .with_context(|| format!("Failed to get user info from Cognito for sub {sub:?}"))?;

    let user_endpoint = user
        .user_attributes()
        .iter()
        .find(|&attr| attr.name() == "custom:endpoint")
        .map(|attr| attr.value())
        .flatten();

    if let Some(user_endpoint) = user_endpoint {
        if user_endpoint != endpoint {
            warn!("User {sub:?} attempted to access endpoint {endpoint:?} but is only authorized for {user_endpoint:?}");
            forbidden!("User is not authorized to access this endpoint");
        }
    }

    let account_id = match user
        .user_attributes()
        .iter()
        .find(|&attr| attr.name() == "custom:account_id")
        .map(|attr| attr.value())
        .flatten()
        .map(|account_id| {
            account_id
                .parse::<u64>()
                .with_context(|| format!("Failed to parse account_id {account_id:?}"))
        }) {
        Some(Ok(account_id)) => Some(account_id),
        Some(Err(err)) => bail!(err),
        None => None,
    };

    info!("Authenticated as {sub:?} with account_id {account_id:?}");

    req.extensions_mut().insert(Principal { account_id });

    Ok(next.run(req).await)
}
