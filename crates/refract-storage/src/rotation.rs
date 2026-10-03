use super::*;

impl Store {
    /// Rotate a bounded batch in the active scope. Repeating this operation is safe.
    /// Old keys must remain configured until payloads, downstream objects and backups expire.
    pub async fn rotate_encryption(&self, limit: i64) -> Result<u64> {
        ensure!(
            (1..=1000).contains(&limit),
            "rotation limit must be 1..1000"
        );
        let Some(key) = &self.options.encryption else {
            return Err(anyhow!("encryption is not configured"));
        };
        let prefix = key.active_prefix();
        let mut total = 0;
        for (table, column, first, second) in [
            ("runs", "execution", "id", "''"),
            ("run_embeddings", "embedding", "run_id", "model"),
            ("trace_spans", "payload", "trace_id", "span_id"),
            ("telemetry", "payload", "id", "''"),
            ("scim_directories", "payload", "id", "''"),
            ("browser_sessions", "payload", "id", "''"),
        ] {
            let mut tx = self.transaction().await?;
            let query = format!(
                "SELECT {first} AS a,{second} AS b,{column} AS payload FROM {table} WHERE organization=$1 AND project=$2 AND environment=$3 AND SUBSTR({column},1,LENGTH($4))<>$4 ORDER BY {first} LIMIT $5"
            );
            let rows = sqlx::query(&query)
                .bind(&self.scope.organization)
                .bind(&self.scope.project)
                .bind(&self.scope.environment)
                .bind(&prefix)
                .bind(limit)
                .fetch_all(&mut *tx)
                .await?;
            for row in rows {
                let a: String = row.try_get("a")?;
                let b: String = row.try_get("b")?;
                let old: String = row.try_get("payload")?;
                let identity = match table {
                    "runs" => a.clone(),
                    "telemetry" => format!("telemetry:{a}"),
                    "scim_directories" => format!("directory:{a}"),
                    "browser_sessions" => format!("session:{a}"),
                    "run_embeddings" => format!("embedding:{a}:{b}"),
                    _ => format!("trace:{a}:{b}"),
                };
                let plaintext = if old.starts_with("enc:") {
                    key.open(&self.scope, &identity, &old)?
                } else {
                    old.clone()
                };
                let encoded = key.seal(&self.scope, &identity, &plaintext)?;
                let update = format!(
                    "UPDATE {table} SET {column}=$4 WHERE organization=$1 AND project=$2 AND environment=$3 AND {first}=$5 AND {second}=$6 AND {column}=$7"
                );
                let changed = sqlx::query(&update)
                    .bind(&self.scope.organization)
                    .bind(&self.scope.project)
                    .bind(&self.scope.environment)
                    .bind(encoded)
                    .bind(&a)
                    .bind(&b)
                    .bind(old)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                total += changed;
                if table == "runs" && changed == 1 {
                    for target in &self.options.outbox_targets {
                        self.enqueue(&mut tx, &a, target, "put").await?;
                        // Invalidate any old lease; retain its deadline before publishing new ciphertext.
                        sqlx::query("UPDATE outbox SET lease_token='' WHERE organization=$1 AND project=$2 AND environment=$3 AND run_id=$4 AND target=$5 AND operation='put'")
                            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                            .bind(&a).bind(target).execute(&mut *tx).await?;
                    }
                }
            }
            tx.commit().await?;
        }
        Ok(total)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[tokio::test]
    async fn rotation_reencrypts_payloads_and_fences_inflight_delivery() {
        let old = STANDARD.encode([1; 32]);
        let next = STANDARD.encode([2; 32]);
        let store = Store::open_with_options(
            "sqlite::memory:",
            StoreOptions {
                encryption: Some(Encryption::from_base64(&old).unwrap()),
                outbox_targets: vec!["s3".into()],
            },
        )
        .await
        .unwrap();
        let run = Run::new("rotation-fixture");
        store.insert(&run).await.unwrap();
        store
            .put_embedding(
                &run.id,
                &Embedding {
                    model: "m".into(),
                    values: vec![1.0],
                },
            )
            .await
            .unwrap();
        let job = store.claim_outbox().await.unwrap().unwrap();
        store
            .insert_telemetry(&[TelemetryRecord {
                kind: "logs".into(),
                trace_id: String::new(),
                payload: serde_json::json!({"body":"private telemetry"}),
            }])
            .await
            .unwrap();
        store
            .update_directory("issuer", &Default::default(), |directory| {
                directory.users.insert(
                    "user".into(),
                    serde_json::json!({"externalId":"subject","active":true}),
                );
                Ok(serde_json::Value::Null)
            })
            .await
            .unwrap();
        store
            .create_browser_session(
                "cookie-digest",
                &SessionTokens {
                    issuer: "issuer".into(),
                    subject: "subject".into(),
                    access_token: "private-access".into(),
                    refresh_token: Some("private-refresh".into()),
                    access_expires_at: Utc::now().timestamp() + 600,
                },
            )
            .await
            .unwrap();
        let mut updated = store.clone();
        updated.options.encryption = Some(
            Encryption::from_keyring_json(
                &serde_json::json!({"active":"next","keys":{"legacy":old,"next":next}}).to_string(),
            )
            .unwrap(),
        );
        assert_eq!(updated.rotate_encryption(10).await.unwrap(), 5);
        assert_eq!(updated.rotate_encryption(10).await.unwrap(), 0);
        assert_eq!(updated.get(&run.id).await.unwrap().unwrap(), run);
        assert_eq!(
            updated.telemetry("logs", "", 10, 0).await.unwrap()[0].payload["body"],
            "private telemetry"
        );
        assert_eq!(
            updated.directory().await.unwrap().users["user"]["externalId"],
            "subject"
        );
        assert_eq!(
            updated
                .browser_session("cookie-digest")
                .await
                .unwrap()
                .unwrap()
                .1
                .refresh_token
                .as_deref(),
            Some("private-refresh")
        );
        let (payload,): (String,) =
            sqlx::query_as("SELECT payload FROM browser_sessions WHERE id='cookie-digest'")
                .fetch_one(&updated.pool)
                .await
                .unwrap();
        assert!(payload.starts_with("enc:v2:next:"));
        assert!(!payload.contains("private-refresh"));
        updated.validate_encryption_keys().await.unwrap();
        assert!(
            updated
                .stored_payload(&run.id)
                .await
                .unwrap()
                .unwrap()
                .starts_with("enc:v2:next:")
        );
        assert!(store.acknowledge(&job).await.is_err());
        assert!(updated.claim_outbox().await.unwrap().is_none());
        sqlx::query("UPDATE outbox SET available_at=0")
            .execute(&updated.pool)
            .await
            .unwrap();
        let rotated = updated.claim_outbox().await.unwrap().unwrap();
        assert_ne!(rotated.id, job.id);
        assert!(rotated.version > job.version);

        assert_eq!(
            updated
                .vector_search(
                    &Embedding {
                        model: "m".into(),
                        values: vec![1.0]
                    },
                    1
                )
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[tokio::test]
    async fn retention_waits_for_inflight_put_lease_before_object_delete() {
        let store = Store::open_with_options(
            "sqlite::memory:",
            StoreOptions {
                outbox_targets: vec!["s3".into()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
        store.insert(&Run::new("retained")).await.unwrap();
        let old = store.claim_outbox().await.unwrap().unwrap();
        store
            .retain_since(Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert!(store.acknowledge(&old).await.is_err());
        assert!(store.claim_outbox().await.unwrap().is_none());
        sqlx::query("UPDATE outbox SET available_at=0")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            store.claim_outbox().await.unwrap().unwrap().operation,
            "delete"
        );
    }
}

impl Store {
    /// Verify every represented key ID, including temporary traces and embeddings, before serving.
    pub(crate) async fn validate_encryption_keys(&self) -> Result<()> {
        let prefixes = self
            .options
            .encryption
            .as_ref()
            .map(Encryption::prefixes)
            .unwrap_or_default();
        for (table, column, a, b) in [
            ("runs", "execution", "id", "''"),
            ("run_embeddings", "embedding", "run_id", "model"),
            ("trace_spans", "payload", "trace_id", "span_id"),
            ("telemetry", "payload", "id", "''"),
            ("scim_directories", "payload", "id", "''"),
            ("browser_sessions", "payload", "id", "''"),
        ] {
            let columns =
                format!("organization,project,environment,{a} AS a,{b} AS b,{column} AS payload");
            let conditions = prefixes
                .iter()
                .enumerate()
                .map(|(i, _)| format!("SUBSTR({column},1,LENGTH(${}))=${}", i + 1, i + 1))
                .collect::<Vec<_>>()
                .join(" OR ");
            let unknown = format!(
                "SELECT {columns} FROM {table} WHERE {column} LIKE 'enc:%'{} LIMIT 1",
                if conditions.is_empty() {
                    String::new()
                } else {
                    format!(" AND NOT ({conditions})")
                }
            );
            let mut query = sqlx::query(&unknown);
            for prefix in &prefixes {
                query = query.bind(prefix);
            }
            ensure!(
                query.fetch_optional(&self.pool).await?.is_none(),
                "stored payload requires an unavailable encryption key"
            );
            for prefix in &prefixes {
                let sample = format!(
                    "SELECT {columns} FROM {table} WHERE SUBSTR({column},1,LENGTH($1))=$1 LIMIT 1"
                );
                if let Some(row) = sqlx::query(&sample)
                    .bind(prefix)
                    .fetch_optional(&self.pool)
                    .await?
                {
                    let scope = Scope {
                        organization: row.try_get("organization")?,
                        project: row.try_get("project")?,
                        environment: row.try_get("environment")?,
                    };
                    let a: String = row.try_get("a")?;
                    let b: String = row.try_get("b")?;
                    let id = match table {
                        "runs" => a,
                        "telemetry" => format!("telemetry:{a}"),
                        "scim_directories" => format!("directory:{a}"),
                        "browser_sessions" => format!("session:{a}"),
                        "run_embeddings" => format!("embedding:{a}:{b}"),
                        _ => format!("trace:{a}:{b}"),
                    };
                    self.options
                        .encryption
                        .as_ref()
                        .expect("prefixes require keys")
                        .open(&scope, &id, row.try_get("payload")?)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[tokio::test]
    async fn startup_rejects_missing_embedding_key_even_when_run_key_is_present() {
        let old = STANDARD.encode([1; 32]);
        let next = STANDARD.encode([2; 32]);
        let mut store = Store::open_with_options(
            "sqlite::memory:",
            StoreOptions {
                encryption: Some(
                    Encryption::from_keyring_json(
                        &serde_json::json!({"active":"old","keys":{"old":old}}).to_string(),
                    )
                    .unwrap(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let run = Run::new("mixed-keys");
        store.insert(&run).await.unwrap();
        store
            .put_embedding(
                &run.id,
                &Embedding {
                    model: "m".into(),
                    values: vec![1.0],
                },
            )
            .await
            .unwrap();
        store.options.encryption = Some(
            Encryption::from_keyring_json(
                &serde_json::json!({"active":"next","keys":{"old":old,"next":next}}).to_string(),
            )
            .unwrap(),
        );
        let payload = store.encode(&run).unwrap();
        sqlx::query("UPDATE runs SET execution=$1")
            .bind(payload)
            .execute(&store.pool)
            .await
            .unwrap();
        store.validate_encryption_keys().await.unwrap();
        store.options.encryption = Some(
            Encryption::from_keyring_json(
                &serde_json::json!({"active":"next","keys":{"next":next}}).to_string(),
            )
            .unwrap(),
        );
        assert!(store.validate_encryption_keys().await.is_err());
    }
}
