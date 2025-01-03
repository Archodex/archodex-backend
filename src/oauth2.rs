use anyhow::Context;
use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{AppendHeaders, IntoResponse},
    Json,
};
use axum_extra::extract::CookieJar;
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::{env::Env, macros::*, PublicError, Result};

#[derive(Deserialize)]
pub(crate) struct IdpResponseQueryParams {
    code: String,
    state: String,
}

#[derive(Deserialize)]
struct CognitoAuthorizeResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct CognitoRefreshResponse {
    access_token: String,
}

pub(crate) async fn idp_response(
    Query(IdpResponseQueryParams { code, state }): Query<IdpResponseQueryParams>,
) -> Result<impl IntoResponse> {
    let client = reqwest::Client::new();

    // e.g. https://auth.archodex.com/oauth2/token
    let mut cognito_token_endpoint = Env::cognito_auth_endpoint().clone();
    cognito_token_endpoint.set_path("/oauth2/token");

    let client_id = Env::cognito_client_id();
    let redirect_uri = Env::cognito_redirect_uri();
    let refresh_token_validity_in_days = Env::cognito_refresh_token_validity_in_days();

    debug!("Making request to {cognito_token_endpoint} for tokens...");

    let response = client
        .post(cognito_token_endpoint)
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

    let CognitoAuthorizeResponse {
        access_token,
        refresh_token,
    } = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse Cognito response as JSON: {body}"))?;

    let access_token_exp =
        exp_from_jwt_token(&access_token).context("Failed to parse access token")?;

    let refresh_token_exp =
        Utc::now() + chrono::Duration::days(refresh_token_validity_in_days as i64);

    info!("Decoded access token with expiration {access_token_exp}, and refresh token with expiration {refresh_token_exp}");

    let mut app_redirect_uri = Env::app_redirect_uri().clone();
    app_redirect_uri
        .query_pairs_mut()
        .append_pair("access_token_expiration", &access_token_exp.to_string())
        .append_pair(
            "refresh_token_expiration",
            &refresh_token_exp.timestamp().to_string(),
        )
        .append_pair("state", &state);

    Ok((
        StatusCode::FOUND,
        AppendHeaders([
            (
                header::SET_COOKIE,
                format!("accessToken={access_token}; HttpOnly; Path=/; SameSite=Strict; Secure"),
            ),
            (
                header::SET_COOKIE,
                format!(
                    "refreshToken={refresh_token}; HttpOnly; Path=/oauth2; SameSite=Strict; Secure"
                ),
            ),
            (header::LOCATION, app_redirect_uri.to_string()),
        ]),
    ))
}

#[derive(Serialize)]
struct RefreshTokenResponse {
    access_token_expiration: u64,
}

pub(crate) async fn refresh_token(cookies: CookieJar) -> Result<impl IntoResponse> {
    let refresh_token = cookies
        .get("refreshToken")
        .ok_or_else(|| {
            anyhow!(PublicError::new(
                StatusCode::BAD_REQUEST,
                "Missing refreshToken cookie"
            ))
        })?
        .value();

    let client = reqwest::Client::new();

    // e.g. https://auth.archodex.com/oauth2/token
    let mut cognito_token_endpoint = Env::cognito_auth_endpoint().clone();
    cognito_token_endpoint.set_path("/oauth2/token");

    let client_id = Env::cognito_client_id();

    debug!("Making request to {cognito_token_endpoint} for refreshed tokens...");

    let response = client
        .post(cognito_token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client_id),
            ("refresh_token", refresh_token),
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
        "Failed to get refreshed tokens from Cognito: {status}:\n{body}",
    );

    let CognitoRefreshResponse { access_token } = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse Cognito response as JSON: {body}"))?;

    let access_token_exp =
        exp_from_jwt_token(&access_token).context("Failed to parse access token")?;

    info!("Decoded access token with expiration {access_token_exp}");

    Ok((
        StatusCode::OK,
        AppendHeaders([(
            header::SET_COOKIE,
            format!("accessToken={access_token}; HttpOnly; Path=/; SameSite=Strict; Secure"),
        )]),
        Json(RefreshTokenResponse {
            access_token_expiration: access_token_exp,
        }),
    ))
}

pub(crate) async fn revoke_token(cookies: CookieJar) -> impl IntoResponse {
    if let Err(err) = try_revoke_token(cookies).await {
        warn!("Failed to revoke token from Cognito service: {err:#?}");
    }

    (
        StatusCode::OK,
        AppendHeaders([
            (
                header::SET_COOKIE,
                "refreshToken=; HttpOnly; Path=/; SameSite=Strict; Secure; Max-Age=0",
            ),
            (
                header::SET_COOKIE,
                "accessToken=; HttpOnly; Path=/; SameSite=Strict; Secure; Max-Age=0",
            ),
        ]),
    )
}

async fn try_revoke_token(cookies: CookieJar) -> anyhow::Result<()> {
    let refresh_token = cookies
        .get("refreshToken")
        .ok_or_else(|| {
            anyhow!(PublicError::new(
                StatusCode::BAD_REQUEST,
                "Missing refreshToken cookie"
            ))
        })?
        .value();

    let client = reqwest::Client::new();

    // e.g. https://auth.archodex.com/oauth2/token
    let mut cognito_revoke_endpoint = Env::cognito_auth_endpoint().clone();
    cognito_revoke_endpoint.set_path("/oauth2/revoke");

    let client_id = Env::cognito_client_id();

    debug!("Making request to {cognito_revoke_endpoint} to revoke token...");

    let response = client
        .post(cognito_revoke_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("client_id", client_id), ("token", refresh_token)])
        .send()
        .await
        .context("Failed to send request to Cognito revoke endpoint")?;

    let status = response.status();

    let body = response
        .text()
        .await
        .context("Failed to parse response body")?;

    ensure!(
        status.is_success(),
        "Received unsuccessful response: {status}:\n{body}",
    );

    Ok(())
}

#[derive(Deserialize)]
struct JwtClaims {
    exp: u64,
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
