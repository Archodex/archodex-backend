use anyhow::Context;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{AppendHeaders, IntoResponse},
};
use base64::Engine;
use chrono::Utc;
use reqwest::Url;
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::{macros::*, Result};

#[derive(Deserialize)]
pub(crate) struct IdpResponseQueryParams {
    code: String,
}

#[derive(Deserialize)]
struct CognitoResponse {
    access_token: String,
    refresh_token: String,
    id_token: String,
}

#[derive(Deserialize)]
struct JwtClaims {
    exp: u64,
}

#[derive(Deserialize)]
struct ArchodexIdTokenClaims {
    #[serde(rename = "custom:endpoint")]
    endpoint: Option<String>,
}

pub(crate) async fn idp_response(
    Query(IdpResponseQueryParams { code }): Query<IdpResponseQueryParams>,
) -> Result<impl IntoResponse> {
    let client = reqwest::Client::new();

    // e.g. https://auth.archodex.com/oauth2/token
    let mut url = Url::parse(
        &std::env::var("COGNITO_TOKEN_ENDPOINT")
            .context("Missing COGNITO_TOKEN_ENDPOINT env var")?,
    )
    .context("Failed to parse env var COGNITO_TOKEN_ENDPOINT as a URL")?;
    url.set_path("/oauth2/token");

    let client_id =
        std::env::var("COGNITO_CLIENT_ID").context("Missing COGNITO_CLIENT_ID env var")?;
    let redirect_uri =
        std::env::var("COGNITO_REDIRECT_URI").context("Missing COGNITO_REDIRECT_URI env var")?;
    let refresh_token_validity_in_days = std::env::var("COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS")
        .context("Missing COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS env var")?;
    let refresh_token_validity_in_days = refresh_token_validity_in_days
        .parse::<u16>()
        .with_context(|| format!("Failed to parse COGNITO_REFRESH_TOKEN_VALIDITY_IN_DAYS as a u16 (value: {refresh_token_validity_in_days:?}"))?;
    let app_redirect_uri =
        std::env::var("APP_REDIRECT_URI").context("Missing APP_REDIRECT_URI env var")?;
    let mut app_redirect_uri = app_redirect_uri.parse::<Url>().with_context(|| {
        format!("Failed to parse APP_REDIRECT_URI as a URL ({app_redirect_uri:?})")
    })?;

    debug!("Making request to {url} for tokens...");

    let response = client
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("redirect_uri", &redirect_uri),
            ("code", &code),
        ])
        .send()
        .await
        .context("Failed to send request to Cognito token endpoint")?;

    let status = response.status();

    let body = response
        .text()
        .await
        .context("Failed to parse response body")?;

    ensure!(
        status.is_success(),
        "Failed to get tokens from Cognito: {status}:\n{body}",
    );

    let CognitoResponse {
        access_token,
        refresh_token,
        id_token,
    } = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse Cognito response as JSON: {body}"))?;

    warn!("Received ID token: {id_token}");

    let endpoint = endpoint_from_id_token(&id_token)?;
    let access_token_exp =
        exp_from_jwt_token(&access_token).context("Failed to parse access token")?;

    let refresh_token_exp =
        Utc::now() + chrono::Duration::days(refresh_token_validity_in_days as i64);

    info!("Decoded ID token with endpoint {endpoint:?}, access token with expiration {access_token_exp}, and refresh token with expiration {refresh_token_exp}");

    app_redirect_uri
        .query_pairs_mut()
        .append_pair("endpoint", &endpoint.unwrap_or_default())
        .append_pair("access_token_expiration", &access_token_exp.to_string())
        .append_pair(
            "refresh_token_expiration",
            &refresh_token_exp.timestamp().to_string(),
        );

    Ok((
        StatusCode::FOUND,
        AppendHeaders([
            (
                "Set-Cookie",
                format!(
                    "accessToken={access_token}; HttpOnly; Path=/; SameSite=Strict; Secure"
                ),
            ),
            (
                "Set-Cookie",
                format!(
                    "refreshToken={refresh_token}; HttpOnly; Path=/oauth2/token; SameSite=Strict; Secure"
                ),
            ),
            (
                "Location",
                app_redirect_uri.to_string(),
            )
        ]),
    ))
}

fn endpoint_from_id_token(id_token: &str) -> anyhow::Result<Option<String>> {
    let parts = id_token.split('.').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "Invalid ID token: {id_token:?}",);

    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1].as_bytes())
        .with_context(|| {
            format!(
                "Failed to decode ID token payload as URL-safe base64 (payload: {:?})",
                parts[1]
            )
        })?;

    let payload = std::str::from_utf8(&payload).with_context(|| {
        format!("Failed to decode ID token payload as UTF-8 (payload: {payload:?})")
    })?;

    let ArchodexIdTokenClaims { endpoint } = serde_json::from_str(payload)
        .with_context(|| format!("ID token has invalid 'endpoint' claim (payload: {payload:?})"))?;

    Ok(endpoint)
}

fn exp_from_jwt_token(jwt_token: &str) -> anyhow::Result<u64> {
    let parts = jwt_token.split('.').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "Invalid JWT token: {jwt_token:?}",);

    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1].as_bytes())
        .with_context(|| {
            format!(
                "Failed to decode JWT token payload as URL-safe base64 (payload: {:?})",
                parts[1]
            )
        })?;

    let payload = std::str::from_utf8(&payload).with_context(|| {
        format!("Failed to decode JWT token payload as UTF-8 (payload: {payload:?})")
    })?;

    let JwtClaims { exp } = serde_json::from_str(payload).with_context(|| {
        format!("JWT token has missing or invalid 'exp' claim (payload: {payload:?})")
    })?;

    Ok(exp)
}
