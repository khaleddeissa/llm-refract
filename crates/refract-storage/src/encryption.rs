use crate::Scope;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Encryption {
    active: String,
    keys: BTreeMap<String, Aes256Gcm>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Keyring {
    active: String,
    keys: BTreeMap<String, String>,
}
impl Encryption {
    fn key(value: &str) -> Result<Aes256Gcm> {
        let key = STANDARD.decode(value)?;
        ensure!(
            key.len() == 32,
            "storage encryption key must contain 32 bytes"
        );
        Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("invalid encryption key"))
    }
    /// A random 32-byte key, retaining the v1 envelope for existing single-key installations.
    pub fn from_base64(value: &str) -> Result<Self> {
        Ok(Self {
            active: "legacy".into(),
            keys: BTreeMap::from([("legacy".into(), Self::key(value)?)]),
        })
    }
    /// Key IDs identify decryptable history; only the active key encrypts new payloads.
    pub fn from_keyring_json(value: &str) -> Result<Self> {
        let ring: Keyring = serde_json::from_str(value)?;
        ensure!(
            (1..=32).contains(&ring.keys.len()),
            "keyring must contain 1..32 keys"
        );
        ensure!(
            ring.keys.contains_key(&ring.active),
            "active encryption key is missing"
        );
        let mut keys = BTreeMap::new();
        for (id, value) in ring.keys {
            ensure!(
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "invalid encryption key id"
            );
            keys.insert(id, Self::key(&value)?);
        }
        Ok(Self {
            active: ring.active,
            keys,
        })
    }
    pub(crate) fn prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> =
            self.keys.keys().map(|id| format!("enc:v2:{id}:")).collect();
        if self.keys.contains_key("legacy") {
            prefixes.push("enc:v1:".into());
        }
        prefixes
    }
    pub(crate) fn active_prefix(&self) -> String {
        if self.active == "legacy" {
            "enc:v1:".into()
        } else {
            format!("enc:v2:{}:", self.active)
        }
    }
    pub fn seal(&self, scope: &Scope, id: &str, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let aad = if self.active == "legacy" {
            serde_json::to_vec(&(scope, id))?
        } else {
            serde_json::to_vec(&(scope, id, &self.active))?
        };
        let ciphertext = self.keys[&self.active]
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("storage encryption failed"))?;
        let mut data = nonce.to_vec();
        data.extend(ciphertext);
        Ok(format!("{}{}", self.active_prefix(), STANDARD.encode(data)))
    }
    pub fn open(&self, scope: &Scope, id: &str, value: &str) -> Result<String> {
        let (key_id, body, legacy) = if let Some(body) = value.strip_prefix("enc:v1:") {
            ("legacy", body, true)
        } else if let Some(body) = value.strip_prefix("enc:v2:") {
            let (key_id, body) = body
                .split_once(':')
                .ok_or_else(|| anyhow!("invalid encrypted envelope"))?;
            (key_id, body, false)
        } else {
            return Err(anyhow!("unknown encrypted envelope"));
        };
        let key = self
            .keys
            .get(key_id)
            .ok_or_else(|| anyhow!("encrypted payload references an unavailable key"))?;
        let data = STANDARD.decode(body)?;
        ensure!(data.len() >= 28, "truncated encrypted snapshot");
        let aad = if legacy {
            serde_json::to_vec(&(scope, id))?
        } else {
            serde_json::to_vec(&(scope, id, key_id))?
        };
        let plaintext = key
            .decrypt(
                Nonce::from_slice(&data[..12]),
                Payload {
                    msg: &data[12..],
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("snapshot authentication failed; check storage key and scope"))?;
        Ok(String::from_utf8(plaintext)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyring_preserves_legacy_and_authenticates_key_identifiers() {
        let legacy = STANDARD.encode([7; 32]);
        let current = STANDARD.encode([8; 32]);
        let old = Encryption::from_base64(&legacy).unwrap();
        let ring = Encryption::from_keyring_json(
            &serde_json::json!({"active":"next","keys":{"legacy":legacy,"next":current}})
                .to_string(),
        )
        .unwrap();
        let original = old
            .seal(&Scope::default(), "run", "secret evidence")
            .unwrap();
        assert_eq!(
            ring.open(&Scope::default(), "run", &original).unwrap(),
            "secret evidence"
        );
        let encrypted = ring.seal(&Scope::default(), "run", "new evidence").unwrap();
        assert!(encrypted.starts_with("enc:v2:next:"));
        assert!(old.open(&Scope::default(), "run", &encrypted).is_err());
        assert!(
            ring.open(
                &Scope::default(),
                "run",
                &encrypted.replace("next:", "legacy:")
            )
            .is_err()
        );
    }
}
