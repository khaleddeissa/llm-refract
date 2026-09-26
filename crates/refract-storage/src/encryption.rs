use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};

use crate::Scope;

#[derive(Clone)]
pub struct Encryption(Aes256Gcm);

impl Encryption {
    /// A base64-encoded, randomly generated 32-byte key; never an application password.
    pub fn from_base64(value: &str) -> Result<Self> {
        let key = STANDARD.decode(value)?;
        ensure!(
            key.len() == 32,
            "storage encryption key must contain 32 bytes"
        );
        Ok(Self(
            Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("invalid encryption key"))?,
        ))
    }

    pub fn seal(&self, scope: &Scope, id: &str, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let aad = serde_json::to_vec(&(scope, id))?;
        let ciphertext = self
            .0
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
        Ok(format!("enc:v1:{}", STANDARD.encode(data)))
    }

    pub fn open(&self, scope: &Scope, id: &str, value: &str) -> Result<String> {
        let data = STANDARD.decode(
            value
                .strip_prefix("enc:v1:")
                .ok_or_else(|| anyhow!("unknown encrypted envelope"))?,
        )?;
        ensure!(data.len() >= 28, "truncated encrypted snapshot");
        let aad = serde_json::to_vec(&(scope, id))?;
        let plaintext = self
            .0
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
