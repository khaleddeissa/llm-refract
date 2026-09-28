use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Embedding {
    pub model: String,
    pub values: Vec<f64>,
}
impl Embedding {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.model.trim().is_empty() && self.model.len() <= 256,
            "model must contain 1..256 bytes"
        );
        ensure!(
            (1..=4096).contains(&self.values.len()),
            "embedding requires 1..4096 dimensions"
        );
        ensure!(
            self.values.iter().all(|v| v.is_finite() && v.abs() <= 1e10),
            "embedding contains invalid values"
        );
        ensure!(
            self.values.iter().any(|v| *v != 0.0),
            "zero vectors have no cosine similarity"
        );
        Ok(())
    }
    fn normalized(&self) -> Vec<f64> {
        let norm = self.values.iter().map(|v| v * v).sum::<f64>().sqrt();
        self.values.iter().map(|v| v / norm).collect()
    }
}
#[derive(Debug, Serialize)]
pub struct VectorMatch {
    pub run_id: String,
    pub score: f64,
}

impl Store {
    /// Atomic, fixed-minute quota shared by all replicas using this store.
    pub async fn allow_request(&self, actor: &str, limit: u32) -> Result<bool> {
        let window_start = Utc::now().timestamp() / 60;
        let (count,): (i64,) = sqlx::query_as("INSERT INTO rate_buckets(organization,project,environment,actor,window_start,requests) VALUES($1,$2,$3,$4,$5,1) ON CONFLICT(organization,project,environment,actor) DO UPDATE SET requests=CASE WHEN rate_buckets.window_start=excluded.window_start THEN rate_buckets.requests+1 ELSE 1 END, window_start=excluded.window_start RETURNING requests")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(actor).bind(window_start).fetch_one(&mut *self.connection().await?).await?;
        Ok(count <= i64::from(limit))
    }
    pub async fn expire_rate_buckets(&self) -> Result<u64> {
        Ok(
            sqlx::query("DELETE FROM rate_buckets WHERE window_start < $1")
                .bind(Utc::now().timestamp() / 60 - 2)
                .execute(&mut *self.connection().await?)
                .await?
                .rows_affected(),
        )
    }
    /// Expire audit records independently of run retention, always within the active scope.
    pub async fn expire_audit(&self, before: DateTime<Utc>) -> Result<u64> {
        ensure!(before < Utc::now(), "audit cutoff must be in the past");
        Ok(sqlx::query("DELETE FROM audit_log WHERE organization=$1 AND project=$2 AND environment=$3 AND timestamp < $4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(before.to_rfc3339()).execute(&mut *self.connection().await?).await?.rows_affected())
    }
    pub async fn put_embedding(&self, run_id: &str, embedding: &Embedding) -> Result<bool> {
        embedding.validate()?;
        let identity = format!("embedding:{run_id}:{}", embedding.model);
        let payload = serde_json::to_string(&embedding.normalized())?;
        let encoded = match &self.options.encryption {
            Some(key) => key.seal(&self.scope, &identity, &payload)?,
            None => payload,
        };
        // INSERT SELECT keeps the existence check and write atomic, including retention races.
        let result = sqlx::query("INSERT INTO run_embeddings(organization,project,environment,run_id,model,dimensions,embedding) SELECT organization,project,environment,id,$5,$6,$7 FROM runs WHERE organization=$1 AND project=$2 AND environment=$3 AND id=$4 ON CONFLICT(organization,project,environment,run_id,model) DO UPDATE SET dimensions=excluded.dimensions,embedding=excluded.embedding")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(run_id).bind(&embedding.model).bind(embedding.values.len() as i64).bind(encoded)
            .execute(&mut *self.connection().await?).await?;
        Ok(result.rows_affected() == 1)
    }
    /// Exact cosine search within one model/dimension namespace and tenant scope.
    /// Refuse oversized candidate sets rather than silently searching an arbitrary subset.
    pub async fn vector_search(&self, query: &Embedding, limit: usize) -> Result<Vec<VectorMatch>> {
        query.validate()?;
        ensure!(
            (1..=100).contains(&limit),
            "vector result limit must be 1..100"
        );
        let rows = sqlx::query("SELECT run_id,embedding FROM run_embeddings WHERE organization=$1 AND project=$2 AND environment=$3 AND model=$4 AND dimensions=$5 ORDER BY run_id LIMIT 10001")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(&query.model).bind(query.values.len() as i64).fetch_all(&mut *self.connection().await?).await?;
        ensure!(
            rows.len() <= 10000,
            "exact vector search supports at most 10000 candidates per namespace"
        );
        let normalized = query.normalized();
        let mut matches = Vec::with_capacity(rows.len());
        for row in rows {
            let run_id: String = row.try_get("run_id")?;
            let payload: String = row.try_get("embedding")?;
            let decoded = if payload.starts_with("enc:") {
                self.options
                    .encryption
                    .as_ref()
                    .ok_or_else(|| anyhow!("embedding key is unavailable"))?
                    .open(
                        &self.scope,
                        &format!("embedding:{run_id}:{}", query.model),
                        &payload,
                    )?
            } else {
                payload
            };
            let values: Vec<f64> = serde_json::from_str(&decoded)?;
            ensure!(
                values.len() == normalized.len(),
                "stored embedding dimension mismatch"
            );
            let score = values
                .iter()
                .zip(&normalized)
                .map(|(a, b)| a * b)
                .sum::<f64>();
            matches.push(VectorMatch {
                run_id,
                score: score.clamp(-1.0, 1.0),
            });
        }
        matches.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.run_id.cmp(&b.run_id))
        });
        matches.truncate(limit);
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn quotas_are_shared_across_clones_and_isolated_by_scope() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let other = store.scoped(Scope {
            organization: "other".into(),
            ..Scope::default()
        });
        let mut tasks = Vec::new();
        for _ in 0..30 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store.allow_request("same-key", 10).await.unwrap()
            }));
        }
        let mut allowed = 0;
        for task in tasks {
            allowed += usize::from(task.await.unwrap());
        }
        assert_eq!(allowed, 10);
        assert!(other.allow_request("same-key", 10).await.unwrap());
    }
    #[tokio::test]
    async fn vectors_validate_rank_and_isolate_model_and_tenant() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let mut run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        for (id, vector) in [("a", vec![1.0, 0.0]), ("b", vec![0.0, 1.0])] {
            run.id = id.into();
            for event in &mut run.events {
                event.run_id = id.into();
            }
            store.insert(&run).await.unwrap();
            assert!(
                store
                    .put_embedding(
                        id,
                        &Embedding {
                            model: "local-v1".into(),
                            values: vector
                        }
                    )
                    .await
                    .unwrap()
            );
        }
        let query = Embedding {
            model: "local-v1".into(),
            values: vec![0.1, 1.0],
        };
        let found = store.vector_search(&query, 2).await.unwrap();
        assert_eq!(found[0].run_id, "b");
        assert!(found[0].score > 0.99);
        let other = store.scoped(Scope {
            project: "other".into(),
            ..Scope::default()
        });
        assert!(other.vector_search(&query, 2).await.unwrap().is_empty());
        assert!(!other.put_embedding("a", &query).await.unwrap());
        assert!(
            store
                .vector_search(
                    &Embedding {
                        model: "other".into(),
                        ..query
                    },
                    2
                )
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            Embedding {
                model: "m".into(),
                values: vec![0.0]
            }
            .validate()
            .is_err()
        );
        assert!(
            Embedding {
                model: "m".into(),
                values: vec![f64::NAN]
            }
            .validate()
            .is_err()
        );
    }
}
