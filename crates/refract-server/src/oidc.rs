//! Validate access tokens against an operator-configured issuer and JWKS endpoint.
use anyhow::{Result, anyhow, ensure};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Deserialize)]
struct Claims {
    sub: String,
}
struct Cache {
    keys: JwkSet,
    fetched: Option<Instant>,
}
#[derive(Clone)]
pub struct Oidc {
    pub issuer: String,
    audience: String,
    uri: Option<reqwest::Url>,
    client: reqwest::Client,
    cache: Arc<Mutex<Cache>>,
}
impl Oidc {
    pub fn from_env() -> Result<Option<Self>> {
        let Some(issuer) = crate::security::secret("REFRACT_OIDC_ISSUER")? else {
            return Ok(None);
        };
        let audience = crate::security::secret("REFRACT_OIDC_AUDIENCE")?
            .ok_or_else(|| anyhow!("OIDC audience is required"))?;
        let uri = crate::security::secret("REFRACT_OIDC_JWKS_URL")?
            .ok_or_else(|| anyhow!("explicit OIDC JWKS URL is required"))?;
        let url = reqwest::Url::parse(&uri)?;
        ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            "OIDC JWKS URL must use HTTPS without credentials or fragment"
        );
        let mut verifier = Self::from_jwks(&issuer, &audience, r#"{"keys":[]}"#)?;
        verifier.uri = Some(url);
        Ok(Some(verifier))
    }
    /// Inject a public JWKS for embedded deployments and offline authentication tests.
    pub fn from_jwks(issuer: &str, audience: &str, jwks: &str) -> Result<Self> {
        ensure!(
            !issuer.is_empty() && !audience.is_empty(),
            "OIDC issuer and audience are required"
        );
        ensure!(jwks.len() <= 1024 * 1024, "JWKS is too large");
        Ok(Self {
            issuer: issuer.into(),
            audience: audience.into(),
            uri: None,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            cache: Arc::new(Mutex::new(Cache {
                keys: serde_json::from_str(jwks)?,
                fetched: None,
            })),
        })
    }
    pub async fn subject(&self, token: &str) -> Result<String> {
        ensure!(token.len() <= 16384, "access token is too large");
        let header = decode_header(token)?;
        ensure!(
            [Algorithm::RS256, Algorithm::ES256, Algorithm::EdDSA].contains(&header.alg),
            "unsupported signing algorithm"
        );
        let kid = header
            .kid
            .ok_or_else(|| anyhow!("token must identify its signing key"))?;
        let mut cache = self.cache.lock().await;
        let unknown = cache.keys.find(&kid).is_none();
        let refresh = cache.fetched.is_none_or(|time| {
            time.elapsed() >= Duration::from_secs(if unknown { 30 } else { 300 })
        });
        if let Some(uri) = &self.uri
            && refresh
        {
            // Back off failures as well as successful refreshes; unknown kid cannot flood an issuer.
            cache.fetched = Some(Instant::now());
            let mut response = self
                .client
                .get(uri.clone())
                .send()
                .await?
                .error_for_status()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 1024 * 1024,
                    "JWKS is too large"
                );
                bytes.extend_from_slice(&chunk);
            }
            let keys: JwkSet = serde_json::from_slice(&bytes)?;
            ensure!(keys.keys.len() <= 100, "too many signing keys");
            cache.keys = keys;
        }
        let jwk = cache
            .keys
            .find(&kid)
            .ok_or_else(|| anyhow!("unknown signing key"))?;
        if let Some(algorithm) = jwk.common.key_algorithm {
            ensure!(
                algorithm.to_string() == format!("{:?}", header.alg),
                "signing algorithm mismatch"
            );
        }
        ensure!(
            jwk.common
                .public_key_use
                .as_ref()
                .is_none_or(|value| *value == jsonwebtoken::jwk::PublicKeyUse::Signature),
            "key is not intended for signatures"
        );
        ensure!(
            jwk.common
                .key_operations
                .as_ref()
                .is_none_or(|ops| ops.contains(&jsonwebtoken::jwk::KeyOperations::Verify)),
            "key does not permit verification"
        );
        let key = DecodingKey::from_jwk(jwk)?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        let claims = decode::<Claims>(token, &key, &validation)?.claims;
        ensure!(
            !claims.sub.is_empty() && claims.sub.len() <= 512,
            "invalid subject"
        );
        Ok(claims.sub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use p256::{
        SecretKey,
        elliptic_curve::{rand_core::OsRng, sec1::ToEncodedPoint},
        pkcs8::{EncodePrivateKey, LineEnding},
    };
    use serde_json::json;

    #[tokio::test]
    async fn signature_issuer_audience_expiry_and_algorithm_are_required() {
        let private = SecretKey::random(&mut OsRng);
        let point = private.public_key().to_encoded_point(false);
        let jwks = json!({"keys":[{"kty":"EC","crv":"P-256","kid":"fixture","use":"sig","alg":"ES256",
            "x":URL_SAFE_NO_PAD.encode(point.x().unwrap()),"y":URL_SAFE_NO_PAD.encode(point.y().unwrap())}]}).to_string();
        let verifier = Oidc::from_jwks("https://issuer.invalid", "refract", &jwks).unwrap();
        let pem = private.to_pkcs8_pem(LineEnding::LF).unwrap();
        let key = EncodingKey::from_ec_pem(pem.as_bytes()).unwrap();
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some("fixture".into());
        let claims = json!({"sub":"user","iss":"https://issuer.invalid","aud":"refract","exp":chrono::Utc::now().timestamp()+600});
        let token = encode(&header, &claims, &key).unwrap();
        assert_eq!(verifier.subject(&token).await.unwrap(), "user");
        for (name, bad) in [
            ("iss", json!("https://evil.invalid")),
            ("aud", json!("other-service")),
            ("exp", json!(1)),
            ("nbf", json!(chrono::Utc::now().timestamp() + 3600)),
        ] {
            let mut changed = claims.clone();
            changed[name] = bad;
            assert!(
                verifier
                    .subject(&encode(&header, &changed, &key).unwrap())
                    .await
                    .is_err()
            );
        }
        let mut missing = claims.clone();
        missing.as_object_mut().unwrap().remove("exp");
        assert!(
            verifier
                .subject(&encode(&header, &missing, &key).unwrap())
                .await
                .is_err()
        );
        header.kid = Some("unknown".into());
        assert!(
            verifier
                .subject(&encode(&header, &claims, &key).unwrap())
                .await
                .is_err()
        );
        let mut symmetric = Header::new(Algorithm::HS256);
        symmetric.kid = Some("fixture".into());
        assert!(
            verifier
                .subject(
                    &encode(&symmetric, &claims, &EncodingKey::from_secret(b"test-only")).unwrap()
                )
                .await
                .is_err()
        );
    }
}
