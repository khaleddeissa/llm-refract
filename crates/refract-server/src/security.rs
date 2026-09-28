use anyhow::{Result, ensure};
use refract_storage::Scope;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, time::Duration};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Reader,
    Writer,
    Admin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyConfig {
    id: String,
    key: String,
    role: Role,
    organization: String,
    project: String,
    environment: String,
}
#[derive(Clone)]
pub struct Identity {
    pub id: String,
    pub scope: Scope,
    pub role: Role,
    digest: [u8; 32],
}
#[derive(Clone)]
pub struct Security {
    pub identities: Vec<Identity>,
    pub requests_per_minute: u32,
    pub retention: Option<Duration>,
    pub oidc: Option<crate::oidc::Oidc>,
    pub login: Option<crate::login::Login>,
}
impl Default for Security {
    fn default() -> Self {
        Self {
            identities: vec![],
            requests_per_minute: 600,
            retention: None,
            oidc: None,
            login: None,
        }
    }
}
impl Security {
    pub fn from_keys(keys: &str) -> Result<Self> {
        let keys: Vec<KeyConfig> = serde_json::from_str(keys)?;
        ensure!(
            !keys.is_empty(),
            "REFRACT_API_KEYS must contain at least one key"
        );
        ensure!(keys.len() <= 10000, "too many API keys");
        let mut ids = HashSet::new();
        let mut digests = HashSet::new();
        let mut identities = vec![];
        for key in keys {
            ensure!(
                !key.id.is_empty() && key.id.len() <= 100,
                "invalid API key id"
            );
            ensure!(
                key.key.len() >= 32
                    && key.key.len() <= 1024
                    && key.key.bytes().all(|b| b.is_ascii_graphic()),
                "API keys must contain 32..1024 printable ASCII characters"
            );
            let digest: [u8; 32] = Sha256::digest(key.key.as_bytes()).into();
            ensure!(
                ids.insert(key.id.clone()) && digests.insert(digest),
                "API key ids and secrets must be unique"
            );
            let scope = Scope {
                organization: key.organization,
                project: key.project,
                environment: key.environment,
            };
            scope.validate()?;
            identities.push(Identity {
                id: key.id,
                digest,
                scope,
                role: key.role,
            });
        }
        Ok(Self {
            identities,
            ..Self::default()
        })
    }
    pub fn from_env() -> Result<Self> {
        let mut security = match secret("REFRACT_API_KEYS")? {
            Some(value) => Self::from_keys(&value)?,
            None => Self::default(),
        };
        security.oidc = crate::oidc::Oidc::from_env()?;
        security.login =
            crate::login::Login::from_env(security.oidc.as_ref().map(|o| o.issuer.as_str()))?;
        ensure!(
            std::env::var("REFRACT_REQUIRE_AUTH").as_deref() != Ok("1")
                || !security.identities.is_empty()
                || security.oidc.is_some(),
            "REFRACT_REQUIRE_AUTH=1 requires API keys or OIDC"
        );
        if let Ok(value) = std::env::var("REFRACT_RATE_LIMIT") {
            security.requests_per_minute = value.parse()?;
            ensure!(
                security.requests_per_minute > 0,
                "REFRACT_RATE_LIMIT must be positive"
            );
        }
        if let Ok(value) = std::env::var("REFRACT_RETENTION_DAYS") {
            let days: u64 = value.parse()?;
            ensure!(
                (1..=36500).contains(&days),
                "retention must be 1..36500 days"
            );
            security.retention = Some(Duration::from_secs(days * 86400));
        }
        Ok(security)
    }
    pub async fn authenticate_store(
        &self,
        store: &refract_storage::Store,
        bearer: Option<&str>,
    ) -> Result<Option<Identity>> {
        if let Some(identity) = self.authenticate(bearer) {
            return Ok(Some(identity));
        }
        let Some(token) = bearer.filter(|value| value.len() <= 16384) else {
            return Ok(None);
        };
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        let principal = if token.starts_with("rfr_") {
            store.resolve_key(&digest).await?
        } else if let Some(oidc) = &self.oidc {
            match oidc.subject(token).await {
                Ok(subject) => store.resolve_subject(&oidc.issuer, &subject).await?,
                Err(_) => None,
            }
        } else {
            None
        };
        principal
            .map(|principal| {
                Ok(Identity {
                    id: principal.id,
                    scope: principal.scope,
                    role: serde_json::from_value(serde_json::Value::String(principal.role))?,
                    digest: [0; 32],
                })
            })
            .transpose()
    }
    pub fn authenticate(&self, bearer: Option<&str>) -> Option<Identity> {
        if self.identities.is_empty() && self.oidc.is_none() {
            return Some(Identity {
                id: "local".into(),
                scope: Scope::default(),
                role: Role::Admin,
                digest: [0; 32],
            });
        }
        let digest: [u8; 32] = Sha256::digest(bearer?.as_bytes()).into();
        // Compare every byte and every configured key. Only hashes remain in server state.
        let mut found = None;
        for identity in &self.identities {
            let delta = identity
                .digest
                .iter()
                .zip(digest)
                .fold(0u8, |a, (b, c)| a | (b ^ c));
            if delta == 0 {
                found = Some(identity.clone());
            }
        }
        found
    }
}

/// Read a secret directly or from a mounted secret file, rejecting ambiguous configuration.
pub(crate) fn secret(name: &str) -> Result<Option<String>> {
    let direct = std::env::var(name).ok();
    let file = std::env::var(format!("{name}_FILE")).ok();
    ensure!(
        direct.is_none() || file.is_none(),
        "set only {name} or {name}_FILE"
    );
    let value = match (direct, file) {
        (Some(value), _) => Some(value),
        (_, Some(path)) => Some(std::fs::read_to_string(path)?.trim_end().to_owned()),
        _ => None,
    };
    ensure!(
        value.as_ref().is_none_or(|v| !v.is_empty()),
        "{name} cannot be empty"
    );
    Ok(value)
}

/// TLS is provided by the deployment ingress; this flag is an operator assertion, not TLS detection.
pub(crate) fn validate_mode(
    mode: &str,
    security: &Security,
    encrypted: bool,
    tls_terminated: bool,
) -> Result<()> {
    ensure!(
        matches!(mode, "local" | "production"),
        "REFRACT_MODE must be local or production"
    );
    if mode == "production" {
        ensure!(
            !security.identities.is_empty() || security.oidc.is_some(),
            "production mode requires REFRACT_API_KEYS or REFRACT_API_KEYS_FILE"
        );
        ensure!(
            encrypted,
            "production mode requires REFRACT_ENCRYPTION_KEY or REFRACT_ENCRYPTION_KEY_FILE"
        );
        ensure!(
            tls_terminated,
            "production mode requires HTTPS at the ingress and REFRACT_TLS_TERMINATED=1"
        );
    }
    Ok(())
}
