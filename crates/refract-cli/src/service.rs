use anyhow::{Result, ensure};
use serde_json::Value;

pub async fn request(path: &str, body: Option<Value>) -> Result<Value> {
    let endpoint =
        std::env::var("REFRACT_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".into());
    let url = reqwest::Url::parse(&endpoint)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "REFRACT_SERVER_URL must be HTTP(S) without credentials, query or fragment"
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(35))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let target = format!("{}{path}", endpoint.trim_end_matches('/'));
    let mut request = match body {
        Some(value) => client.post(target).json(&value),
        None => client.get(target),
    };
    let key = match (
        std::env::var("REFRACT_API_KEY").ok(),
        std::env::var("REFRACT_API_KEY_FILE").ok(),
    ) {
        (Some(_), Some(_)) => {
            anyhow::bail!("set REFRACT_API_KEY or REFRACT_API_KEY_FILE, not both")
        }
        (Some(key), None) => Some(key),
        (None, Some(path)) => Some(std::fs::read_to_string(path)?.trim().to_owned()),
        (None, None) => None,
    };
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await?;
    ensure!(
        response.status().is_success(),
        "Refract API returned HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 17 * 1024 * 1024,
            "service response exceeds 17 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
