//! OIDC code exchange and refresh use fixed operator endpoints. Tokens never enter browser JS.
use super::*;
use anyhow::{Result, anyhow, ensure};
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use refract_storage::SessionTokens;
use sha2::{Digest, Sha256};
const SESSION: &str = "__Host-refract.session";
const LOGIN: &str = "__Host-refract.login";
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn random() -> String {
    format!("{}{}", refract_core::id(""), refract_core::id(""))
}
pub(super) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let values: Vec<_> = headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(key, _)| *key == name)
        .map(|(_, value)| value)
        .collect();
    if values.len() != 1 || values[0].len() > 256 {
        None
    } else {
        Some(values[0].into())
    }
}
fn set_cookie(response: &mut Response, name: &str, value: &str, age: u32) {
    response.headers_mut().append(
        "set-cookie",
        format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={age}")
            .parse()
            .expect("generated cookie"),
    );
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
}
pub(super) fn origin(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let login = state
        .security
        .login
        .as_ref()
        .ok_or_else(|| invalid("SSO is disabled"))?;
    let expected = reqwest::Url::parse(&login.redirect_uri)
        .map_err(invalid)?
        .origin()
        .ascii_serialization();
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(expected.as_str()) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "same-origin request required".into(),
        ));
    }
    Ok(())
}
#[derive(serde::Serialize, Deserialize)]
struct Pending {
    verifier: String,
    issuer: String,
    client_id: String,
    redirect_uri: String,
}
pub(super) async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    origin(&state, &headers)?;
    if !state
        .store
        .allow_request("browser-login", state.security.requests_per_minute)
        .await
        .map_err(internal)?
    {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many sign-in attempts".into(),
        ));
    }
    let login = state.security.login.as_ref().expect("checked login");
    let state_id = random();
    let binding = random();
    let pending = Pending {
        verifier: random(),
        issuer: login.issuer.clone(),
        client_id: login.client_id.clone(),
        redirect_uri: login.redirect_uri.clone(),
    };
    let mut url = reqwest::Url::parse(&login.authorization_endpoint).map_err(invalid)?;
    {
        let mut params = url.query_pairs_mut();
        for (key, value) in &login.authorization_params {
            params.append_pair(key, value);
        }
        for (key, value) in [
            ("response_type", "code"),
            ("client_id", login.client_id.as_str()),
            ("redirect_uri", login.redirect_uri.as_str()),
            ("scope", login.scope.as_str()),
            ("state", state_id.as_str()),
            ("code_challenge_method", "S256"),
        ] {
            params.append_pair(key, value);
        }
        params.append_pair(
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes())),
        );
    }
    state
        .store
        .begin_browser_login(
            &hash(&state_id),
            &hash(&binding),
            &serde_json::to_string(&pending).map_err(internal)?,
        )
        .await
        .map_err(internal)?;
    let mut response = Json(json!({"url":url.as_str()})).into_response();
    set_cookie(&mut response, LOGIN, &binding, 600);
    Ok(response)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Callback {
    state: String,
    code: String,
}
async fn exchange(login: &login::Login, mut form: Vec<(&str, String)>) -> Result<Value> {
    form.push(("client_id", login.client_id.clone()));
    if let Some(secret) = &login.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client
        .post(&login.token_endpoint)
        .form(&form)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "identity provider rejected token exchange"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 65536,
            "token response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn tokens(
    oidc: &oidc::Oidc,
    value: Value,
    previous: Option<&SessionTokens>,
) -> Result<SessionTokens> {
    ensure!(
        value["token_type"]
            .as_str()
            .is_some_and(|v| v.eq_ignore_ascii_case("bearer")),
        "bearer token required"
    );
    let access = value["access_token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 16384)
        .ok_or_else(|| anyhow!("invalid access token"))?;
    let (subject, expiry) = oidc.subject_expiry(access).await?;
    ensure!(
        expiry > chrono::Utc::now().timestamp(),
        "expired access token"
    );
    if let Some(old) = previous {
        ensure!(
            old.subject == subject && old.issuer == oidc.issuer,
            "identity changed during refresh"
        );
    }
    let refresh = match value.get("refresh_token") {
        Some(v) => Some(
            v.as_str()
                .filter(|v| !v.is_empty() && v.len() <= 16384)
                .ok_or_else(|| anyhow!("invalid refresh token"))?
                .to_owned(),
        ),
        None => previous.and_then(|p| p.refresh_token.clone()),
    };
    Ok(SessionTokens {
        issuer: oidc.issuer.clone(),
        subject,
        access_token: access.into(),
        refresh_token: refresh,
        access_expires_at: expiry,
    })
}
pub(super) async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(callback): Json<Callback>,
) -> ApiResult<Response> {
    origin(&state, &headers)?;
    if callback.state.len() > 256 || callback.code.is_empty() || callback.code.len() > 8192 {
        return Err(invalid("invalid callback"));
    }
    let binding = cookie(&headers, LOGIN).ok_or_else(|| invalid("login cookie missing"))?;
    let pending = state
        .store
        .consume_browser_login(&hash(&callback.state), &hash(&binding))
        .await
        .map_err(internal)?
        .ok_or_else(|| invalid("login state is invalid, expired or already used"))?;
    let pending: Pending = serde_json::from_str(&pending).map_err(internal)?;
    let login = state.security.login.as_ref().expect("checked login");
    if pending.issuer != login.issuer
        || pending.client_id != login.client_id
        || pending.redirect_uri != login.redirect_uri
    {
        return Err(invalid("login configuration changed"));
    }
    let oidc = state
        .security
        .oidc
        .as_ref()
        .ok_or_else(|| invalid("OIDC is disabled"))?;
    let value = exchange(
        login,
        vec![
            ("grant_type", "authorization_code".into()),
            ("code", callback.code),
            ("redirect_uri", login.redirect_uri.clone()),
            ("code_verifier", pending.verifier),
        ],
    )
    .await
    .map_err(|_| invalid("SSO token exchange failed"))?;
    let tokens = tokens(oidc, value, None)
        .await
        .map_err(|_| invalid("invalid identity provider token"))?;
    let principal = state
        .store
        .resolve_subject(&oidc.issuer, &tokens.subject)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError(StatusCode::FORBIDDEN, "subject is not provisioned".into()))?;
    let secret = random();
    state
        .store
        .scoped(principal.scope)
        .create_browser_session(&hash(&secret), &tokens)
        .await
        .map_err(internal)?;
    if let Some(old) = cookie(&headers, SESSION) {
        state
            .store
            .delete_browser_session(&hash(&old))
            .await
            .map_err(internal)?;
    }
    let mut response = Json(json!({"authenticated":true})).into_response();
    set_cookie(&mut response, SESSION, &secret, 7 * 86400);
    set_cookie(&mut response, LOGIN, "", 0);
    Ok(response)
}
pub(super) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    origin(&state, &headers)?;
    if let Some(secret) = cookie(&headers, SESSION) {
        state
            .store
            .delete_browser_session(&hash(&secret))
            .await
            .map_err(internal)?;
    }
    let mut response = Json(json!({"authenticated":false})).into_response();
    set_cookie(&mut response, SESSION, "", 0);
    set_cookie(&mut response, LOGIN, "", 0);
    Ok(response)
}
pub(super) async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<security::Identity>> {
    let (Some(secret), Some(login), Some(oidc)) = (
        cookie(headers, SESSION),
        state.security.login.as_ref(),
        state.security.oidc.as_ref(),
    ) else {
        return Ok(None);
    };
    let id = hash(&secret);
    for _ in 0..120 {
        let Some((scope, mut stored)) = state.store.browser_session(&id).await? else {
            return Ok(None);
        };
        if stored.issuer != oidc.issuer
            || state
                .store
                .resolve_subject(&stored.issuer, &stored.subject)
                .await?
                .is_none_or(|p| p.scope != scope)
        {
            state.store.delete_browser_session(&id).await?;
            return Ok(None);
        }
        if stored.access_expires_at <= chrono::Utc::now().timestamp() + 30
            && stored.refresh_token.is_some()
        {
            let lease = random();
            if !state.store.claim_browser_refresh(&id, &lease).await? {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
            // Re-read after claiming: a concurrent request may have refreshed just before our claim.
            let Some((_, latest)) = state.store.browser_session(&id).await? else {
                return Ok(None);
            };
            stored = latest;
            let updated = if stored.access_expires_at > chrono::Utc::now().timestamp() + 30 {
                Ok(stored.clone())
            } else {
                match exchange(
                    login,
                    vec![
                        ("grant_type", "refresh_token".into()),
                        (
                            "refresh_token",
                            stored.refresh_token.clone().unwrap_or_default(),
                        ),
                    ],
                )
                .await
                {
                    Ok(value) => tokens(oidc, value, Some(&stored)).await,
                    Err(error) => Err(error),
                }
            };
            stored = match updated {
                Ok(tokens) => tokens,
                Err(_) => {
                    state.store.delete_browser_session(&id).await?;
                    return Ok(None);
                }
            };
            if !state
                .store
                .scoped(scope)
                .finish_browser_refresh(&id, &lease, &stored)
                .await?
            {
                return Ok(None);
            }
        }
        if stored.access_expires_at <= chrono::Utc::now().timestamp() {
            state.store.delete_browser_session(&id).await?;
            return Ok(None);
        }
        let identity = state
            .security
            .authenticate_store(&state.store, Some(&stored.access_token))
            .await?;
        if identity.is_none() {
            state.store.delete_browser_session(&id).await?;
        } else {
            state.store.touch_browser_session(&id).await?;
        }
        return Ok(identity);
    }
    Err(anyhow!("session refresh is busy; retry request"))
}
#[cfg(test)]
mod tests;
