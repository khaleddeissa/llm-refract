use super::*;

#[derive(Debug, Serialize)]
pub struct Principal {
    pub id: String,
    pub scope: Scope,
    pub role: String,
}
#[derive(Debug, Serialize)]
pub struct KeyMetadata {
    pub id: String,
    pub role: String,
    pub expires_at: i64,
    pub revoked: bool,
    pub created_at: String,
}
fn valid_role(role: &str) -> Result<()> {
    ensure!(
        ["reader", "writer", "admin"].contains(&role),
        "invalid role"
    );
    Ok(())
}
fn principal(row: sqlx::any::AnyRow, id: String) -> Result<Principal> {
    Ok(Principal {
        id,
        scope: Scope {
            organization: row.try_get("organization")?,
            project: row.try_get("project")?,
            environment: row.try_get("environment")?,
        },
        role: row.try_get("role")?,
    })
}
impl Store {
    pub async fn create_key(
        &self,
        id: &str,
        digest: &str,
        role: &str,
        expires_at: i64,
    ) -> Result<()> {
        valid_role(role)?;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid key digest"
        );
        ensure!(
            expires_at > Utc::now().timestamp(),
            "key expiry must be in the future"
        );
        sqlx::query("INSERT INTO managed_keys(id,organization,project,environment,digest,role,expires_at,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(id).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(digest).bind(role).bind(expires_at).bind(Utc::now().to_rfc3339()).execute(&mut *self.connection().await?).await?;
        Ok(())
    }
    /// Authentication registry lookup intentionally precedes tenant scoping.
    pub async fn resolve_key(&self, digest: &str) -> Result<Option<Principal>> {
        sqlx::query("SELECT id,organization,project,environment,role FROM managed_keys WHERE digest=$1 AND revoked=0 AND expires_at>$2")
            .bind(digest).bind(Utc::now().timestamp()).fetch_optional(&mut *self.connection().await?).await?
            .map(|row| { let id = row.try_get("id")?; principal(row, id) }).transpose()
    }
    pub async fn keys(&self) -> Result<Vec<KeyMetadata>> {
        sqlx::query("SELECT id,role,expires_at,revoked,created_at FROM managed_keys WHERE organization=$1 AND project=$2 AND environment=$3 ORDER BY created_at DESC LIMIT 1000")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).fetch_all(&mut *self.connection().await?).await?
            .into_iter().map(|row| Ok(KeyMetadata {id:row.try_get("id")?, role:row.try_get("role")?, expires_at:row.try_get("expires_at")?, revoked:row.try_get::<i64,_>("revoked")? != 0, created_at:row.try_get("created_at")?})).collect()
    }
    pub async fn revoke_key(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("UPDATE managed_keys SET revoked=1 WHERE id=$1 AND organization=$2 AND project=$3 AND environment=$4")
            .bind(id).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .execute(&mut *self.connection().await?).await?.rows_affected() == 1)
    }
    pub async fn provision(
        &self,
        issuer: &str,
        subject: &str,
        role: &str,
        enabled: bool,
    ) -> Result<bool> {
        valid_role(role)?;
        ensure!(
            !issuer.is_empty()
                && issuer.len() <= 2048
                && !subject.is_empty()
                && subject.len() <= 512,
            "invalid issuer or subject"
        );
        // A scoped administrator cannot overwrite another tenant's subject binding.
        Ok(sqlx::query("INSERT INTO principals(issuer,subject,organization,project,environment,role,enabled) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(issuer,subject) DO UPDATE SET role=excluded.role,enabled=excluded.enabled WHERE principals.organization=excluded.organization AND principals.project=excluded.project AND principals.environment=excluded.environment")
            .bind(issuer).bind(subject).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(role).bind(i64::from(enabled)).execute(&mut *self.connection().await?).await?.rows_affected()==1)
    }
    pub async fn resolve_subject(&self, issuer: &str, subject: &str) -> Result<Option<Principal>> {
        sqlx::query("SELECT organization,project,environment,role FROM principals WHERE issuer=$1 AND subject=$2 AND enabled=1")
            .bind(issuer).bind(subject).fetch_optional(&mut *self.connection().await?).await?
            .map(|row| principal(row, format!("oidc:{subject}"))).transpose()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn provisioning_cannot_take_over_another_tenant_and_revocation_is_immediate() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let other = store.scoped(Scope {
            project: "other".into(),
            ..Scope::default()
        });
        assert!(
            store
                .provision("https://issuer.invalid", "user", "reader", true)
                .await
                .unwrap()
        );
        assert!(
            !other
                .provision("https://issuer.invalid", "user", "admin", true)
                .await
                .unwrap()
        );
        assert_eq!(
            store
                .resolve_subject("https://issuer.invalid", "user")
                .await
                .unwrap()
                .unwrap()
                .role,
            "reader"
        );
        store
            .provision("https://issuer.invalid", "user", "reader", false)
            .await
            .unwrap();
        assert!(
            store
                .resolve_subject("https://issuer.invalid", "user")
                .await
                .unwrap()
                .is_none()
        );
        let digest = "a".repeat(64);
        store
            .create_key("test", &digest, "writer", Utc::now().timestamp() + 100)
            .await
            .unwrap();
        assert!(store.resolve_key(&digest).await.unwrap().is_some());
        assert!(!other.revoke_key("test").await.unwrap());
        assert!(store.revoke_key("test").await.unwrap());
        assert!(store.resolve_key(&digest).await.unwrap().is_none());
        assert!(other.keys().await.unwrap().is_empty());
    }
}
