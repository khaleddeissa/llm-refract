//! A scoped directory and its authorization bindings commit as one transaction.
use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Directory {
    pub users: BTreeMap<String, Value>,
    pub groups: BTreeMap<String, Value>,
}
impl Store {
    fn decode_directory(&self, payload: &str) -> Result<Directory> {
        let text = if payload.starts_with("enc:") {
            self.options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("directory encryption key required"))?
                .open(&self.scope, "directory:directory", payload)?
        } else {
            payload.into()
        };
        Ok(serde_json::from_str(&text)?)
    }
    fn encode_directory(&self, directory: &Directory) -> Result<String> {
        let text = serde_json::to_string(directory)?;
        ensure!(text.len() <= 16 * 1024 * 1024, "directory exceeds 16 MiB");
        match &self.options.encryption {
            Some(key) => key.seal(&self.scope, "directory:directory", &text),
            None => Ok(text),
        }
    }
    pub async fn directory(&self) -> Result<Directory> {
        let row = sqlx::query("SELECT payload FROM scim_directories WHERE organization=$1 AND project=$2 AND environment=$3")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .fetch_optional(&mut *self.connection().await?).await?;
        row.map(|r| self.decode_directory(r.try_get("payload")?))
            .transpose()
            .map(Option::unwrap_or_default)
    }
    pub async fn update_directory<F>(
        &self,
        issuer: &str,
        group_roles: &BTreeMap<String, String>,
        change: F,
    ) -> Result<Value>
    where
        F: FnOnce(&mut Directory) -> Result<Value> + Send,
    {
        ensure!(!issuer.is_empty(), "OIDC issuer required");
        ensure!(
            group_roles
                .values()
                .all(|r| ["reader", "writer", "admin"].contains(&r.as_str())),
            "invalid group role"
        );
        let mut tx = self.transaction().await?;
        // Obtain the row's write lock before reading, including for a new directory.
        sqlx::query("INSERT INTO scim_directories(organization,project,environment,payload) VALUES($1,$2,$3,$4) ON CONFLICT(organization,project,environment) DO UPDATE SET id=scim_directories.id")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(self.encode_directory(&Directory::default())?).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT payload FROM scim_directories WHERE organization=$1 AND project=$2 AND environment=$3")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).fetch_one(&mut *tx).await?;
        let mut directory = self.decode_directory(row.try_get("payload")?)?;
        let before = directory.clone();
        let result = change(&mut directory)?;
        ensure!(
            directory.users.len() <= 10000 && directory.groups.len() <= 10000,
            "directory exceeds 10000 users or groups"
        );
        // Disabling removed identities and recomputing group roles is atomic with SCIM changes.
        for user in before.users.values() {
            if !directory
                .users
                .values()
                .any(|v| v["externalId"] == user["externalId"])
            {
                sqlx::query("UPDATE principals SET enabled=0 WHERE issuer=$1 AND subject=$2 AND organization=$3 AND project=$4 AND environment=$5")
                    .bind(issuer).bind(user["externalId"].as_str().ok_or_else(|| anyhow!("missing subject"))?)
                    .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).execute(&mut *tx).await?;
            }
        }
        let mut ranks = BTreeMap::<String, u8>::new();
        for group in directory.groups.values() {
            let rank = match group_roles
                .get(group["displayName"].as_str().unwrap_or(""))
                .map(String::as_str)
            {
                Some("admin") => 3,
                Some("writer") => 2,
                Some("reader") => 1,
                _ => 0,
            };
            if let Some(members) = group["members"].as_array() {
                for member in members {
                    if let Some(id) = member["value"].as_str() {
                        let current = ranks.entry(id.into()).or_default();
                        *current = (*current).max(rank);
                    }
                }
            }
        }
        for (id, user) in &directory.users {
            let rank = ranks
                .get(id)
                .copied()
                .unwrap_or(u8::from(group_roles.is_empty()));
            let role = match rank {
                3 => "admin",
                2 => "writer",
                _ => "reader",
            };
            let subject = user["externalId"]
                .as_str()
                .ok_or_else(|| anyhow!("missing subject"))?;
            let enabled = user["active"].as_bool().unwrap_or(true) && rank > 0;
            let changed = sqlx::query("INSERT INTO principals(issuer,subject,organization,project,environment,role,enabled) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(issuer,subject) DO UPDATE SET role=excluded.role,enabled=excluded.enabled WHERE principals.organization=excluded.organization AND principals.project=excluded.project AND principals.environment=excluded.environment")
                .bind(issuer).bind(subject).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                .bind(role).bind(i64::from(enabled)).execute(&mut *tx).await?.rows_affected();
            ensure!(changed == 1, "subject belongs to another scope");
        }
        sqlx::query("UPDATE scim_directories SET payload=$4 WHERE organization=$1 AND project=$2 AND environment=$3")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(self.encode_directory(&directory)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn group_removal_deactivation_and_cross_scope_conflicts_are_atomic() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let roles = BTreeMap::from([("Editors".into(), "writer".into())]);
        store
            .update_directory("issuer", &roles, |d| {
                d.users
                    .insert("u".into(), json!({"externalId":"subject","active":true}));
                d.groups.insert(
                    "g".into(),
                    json!({"displayName":"Editors","members":[{"value":"u"}]}),
                );
                Ok(Value::Null)
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .resolve_subject("issuer", "subject")
                .await
                .unwrap()
                .unwrap()
                .role,
            "writer"
        );
        let other = store.scoped(Scope {
            project: "other".into(),
            ..Scope::default()
        });
        assert!(
            other
                .update_directory("issuer", &roles, |d| {
                    d.users
                        .insert("evil".into(), json!({"externalId":"subject"}));
                    Ok(Value::Null)
                })
                .await
                .is_err()
        );
        assert!(other.directory().await.unwrap().users.is_empty());
        store
            .update_directory("issuer", &roles, |d| {
                d.groups.clear();
                Ok(Value::Null)
            })
            .await
            .unwrap();
        assert!(
            store
                .resolve_subject("issuer", "subject")
                .await
                .unwrap()
                .is_none()
        );
        store
            .update_directory("issuer", &BTreeMap::new(), |d| {
                d.users.get_mut("u").unwrap()["active"] = json!(false);
                Ok(Value::Null)
            })
            .await
            .unwrap();
        assert!(
            store
                .resolve_subject("issuer", "subject")
                .await
                .unwrap()
                .is_none()
        );
    }
}
