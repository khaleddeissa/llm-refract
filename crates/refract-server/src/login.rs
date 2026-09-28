//! Public-client OIDC configuration; the browser uses authorization code + PKCE.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Serialize)]
pub struct Login {
    pub issuer: String,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub scope: String,
    pub authorization_params: BTreeMap<String, String>,
}
fn required(name: &str) -> Result<String> {
    crate::security::secret(name)?
        .ok_or_else(|| anyhow::anyhow!("{name} is required for browser SSO"))
}
fn https(value: String) -> Result<String> {
    let parsed = reqwest::Url::parse(&value)?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str().is_some()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.fragment().is_none(),
        "OIDC browser endpoints require HTTPS without credentials or fragments"
    );
    Ok(value)
}
impl Login {
    pub fn from_env(issuer: Option<&str>) -> Result<Option<Self>> {
        let Some(client_id) = crate::security::secret("REFRACT_OIDC_CLIENT_ID")? else {
            return Ok(None);
        };
        let issuer = issuer
            .ok_or_else(|| anyhow::anyhow!("configure OIDC verification before browser SSO"))?;
        let redirect_uri = https(required("REFRACT_OIDC_REDIRECT_URI")?)?;
        ensure!(
            reqwest::Url::parse(&redirect_uri)?.query().is_none(),
            "redirect URI must not contain a query"
        );
        let authorization_params: BTreeMap<String, String> = serde_json::from_str(
            &crate::security::secret("REFRACT_OIDC_AUTH_PARAMS")?.unwrap_or("{}".into()),
        )?;
        ensure!(
            authorization_params
                .keys()
                .all(|key| ["audience", "resource"].contains(&key.as_str())),
            "only audience/resource authorization parameters are supported"
        );
        Ok(Some(Self {
            issuer: issuer.into(),
            client_id,
            authorization_endpoint: https(required("REFRACT_OIDC_AUTHORIZATION_URL")?)?,
            token_endpoint: https(required("REFRACT_OIDC_TOKEN_URL")?)?,
            redirect_uri,
            scope: crate::security::secret("REFRACT_OIDC_SCOPES")?
                .unwrap_or("openid profile".into()),
            authorization_params,
        }))
    }
}
pub(super) async fn config(
    axum::extract::State(state): axum::extract::State<super::AppState>,
) -> axum::Json<serde_json::Value> {
    axum::Json(match &state.security.login {
        Some(login) => serde_json::json!({"enabled":true,"configuration":login}),
        None => serde_json::json!({"enabled":false}),
    })
}
