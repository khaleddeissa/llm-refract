//! Encrypted browser credentials. Only SHA-256 cookie digests reach SQL.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
pub struct SessionTokens {
    pub issuer: String,
    pub subject: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub access_expires_at: i64,
}
impl Store {
    pub fn has_encryption(&self) -> bool {
        self.options.encryption.is_some()
    }
    pub async fn begin_browser_login(&self, id: &str, binding: &str, payload: &str) -> Result<()> {
        let key = self
            .options
            .encryption
            .as_ref()
            .ok_or_else(|| anyhow!("SSO sessions require storage encryption"))?;
        let encoded = key.seal(&Scope::default(), &format!("login:{id}"), payload)?;
        let mut tx = self.transaction().await?;
        sqlx::query("DELETE FROM browser_logins WHERE expires_at<$1")
            .bind(Utc::now().timestamp())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM browser_sessions WHERE expires_at<$1 OR last_seen<$2")
            .bind(Utc::now().timestamp())
            .bind(Utc::now().timestamp() - 86400)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO browser_logins(id,binding,payload,expires_at) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(binding)
        .bind(encoded)
        .bind(Utc::now().timestamp() + 600)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn consume_browser_login(&self, id: &str, binding: &str) -> Result<Option<String>> {
        let row = sqlx::query("DELETE FROM browser_logins WHERE id=$1 AND binding=$2 AND expires_at>$3 RETURNING payload")
            .bind(id).bind(binding).bind(Utc::now().timestamp()).fetch_optional(&mut *self.connection().await?).await?;
        row.map(|r| {
            self.options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("session encryption required"))?
                .open(
                    &Scope::default(),
                    &format!("login:{id}"),
                    r.try_get("payload")?,
                )
        })
        .transpose()
    }
    fn encode_session(&self, id: &str, tokens: &SessionTokens) -> Result<String> {
        self.options
            .encryption
            .as_ref()
            .ok_or_else(|| anyhow!("SSO sessions require storage encryption"))?
            .seal(
                &self.scope,
                &format!("session:{id}"),
                &serde_json::to_string(tokens)?,
            )
    }
    pub async fn create_browser_session(&self, id: &str, tokens: &SessionTokens) -> Result<()> {
        sqlx::query("INSERT INTO browser_sessions(id,organization,project,environment,payload,expires_at,last_seen) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(id).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(self.encode_session(id,tokens)?).bind(Utc::now().timestamp()+7*86400).bind(Utc::now().timestamp())
            .execute(&mut *self.connection().await?).await?;
        Ok(())
    }
    /// Pre-authentication lookup, like the managed-key and principal registries.
    pub async fn browser_session(&self, id: &str) -> Result<Option<(Scope, SessionTokens)>> {
        let row = sqlx::query("SELECT organization,project,environment,payload FROM browser_sessions WHERE id=$1 AND expires_at>$2 AND last_seen>$3")
            .bind(id).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()-86400).fetch_optional(&mut *self.connection().await?).await?;
        row.map(|row| {
            let scope = Scope {
                organization: row.try_get("organization")?,
                project: row.try_get("project")?,
                environment: row.try_get("environment")?,
            };
            let payload = self
                .options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("session encryption required"))?
                .open(&scope, &format!("session:{id}"), row.try_get("payload")?)?;
            Ok((scope, serde_json::from_str(&payload)?))
        })
        .transpose()
    }
    pub async fn claim_browser_refresh(&self, id: &str, lease: &str) -> Result<bool> {
        Ok(sqlx::query("UPDATE browser_sessions SET lease=$2,lease_until=$3 WHERE id=$1 AND lease_until<$4 AND expires_at>$4 AND last_seen>$5")
            .bind(id).bind(lease).bind(Utc::now().timestamp()+30).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()-86400)
            .execute(&mut *self.connection().await?).await?.rows_affected() == 1)
    }
    pub async fn finish_browser_refresh(
        &self,
        id: &str,
        lease: &str,
        tokens: &SessionTokens,
    ) -> Result<bool> {
        Ok(sqlx::query("UPDATE browser_sessions SET payload=$3,lease='',lease_until=0,last_seen=$4 WHERE id=$1 AND lease=$2 AND lease_until>$4 AND expires_at>$4 AND organization=$5 AND project=$6 AND environment=$7")
            .bind(id).bind(lease).bind(self.encode_session(id,tokens)?).bind(Utc::now().timestamp())
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).execute(&mut *self.connection().await?).await?.rows_affected() == 1)
    }
    pub async fn touch_browser_session(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE browser_sessions SET last_seen=$2 WHERE id=$1 AND expires_at>$2 AND last_seen>$3")
            .bind(id).bind(Utc::now().timestamp()).bind(Utc::now().timestamp()-86400).execute(&mut *self.connection().await?).await?;
        Ok(())
    }
    pub async fn delete_browser_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM browser_sessions WHERE id=$1")
            .bind(id)
            .execute(&mut *self.connection().await?)
            .await?;
        Ok(())
    }
}
