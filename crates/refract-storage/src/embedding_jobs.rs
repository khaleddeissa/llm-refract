use super::*;

/// Contains identifiers only. Provider endpoints and credentials never enter project settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingSetting {
    pub profile: String,
    pub model: String,
    pub is_default: bool,
    pub auto_index: bool,
}

#[derive(Debug)]
pub struct EmbeddingJob {
    pub scope: Scope,
    pub run_id: String,
    pub profile: String,
    pub model: String,
    pub lease_token: String,
    pub attempts: i64,
}

impl Store {
    pub async fn embedding_settings(&self) -> Result<Vec<EmbeddingSetting>> {
        let rows = sqlx::query("SELECT profile,model,is_default,auto_index FROM project_embeddings WHERE organization=$1 AND project=$2 AND environment=$3 ORDER BY profile")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .fetch_all(&mut *self.connection().await?).await?;
        rows.into_iter()
            .map(|row| {
                Ok(EmbeddingSetting {
                    profile: row.try_get("profile")?,
                    model: row.try_get("model")?,
                    is_default: row.try_get::<i64, _>("is_default")? == 1,
                    auto_index: row.try_get::<i64, _>("auto_index")? == 1,
                })
            })
            .collect()
    }

    /// Atomically replace enabled profiles, cancel obsolete work, and index existing snapshots.
    pub async fn set_embedding_settings(&self, settings: &[EmbeddingSetting]) -> Result<()> {
        ensure!(
            settings.len() <= 16,
            "at most 16 embedding profiles per project"
        );
        ensure!(
            settings.is_empty() || settings.iter().filter(|s| s.is_default).count() == 1,
            "select exactly one default profile"
        );
        let mut seen = std::collections::BTreeSet::new();
        for s in settings {
            ensure!(
                !s.profile.is_empty()
                    && s.profile.len() <= 100
                    && !s.model.is_empty()
                    && s.model.len() <= 256
                    && seen.insert(&s.profile),
                "invalid or duplicate embedding profile"
            );
        }
        let mut tx = self.transaction().await?;
        sqlx::query("DELETE FROM project_embeddings WHERE organization=$1 AND project=$2 AND environment=$3")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).execute(&mut *tx).await?;
        for s in settings {
            sqlx::query("INSERT INTO project_embeddings(organization,project,environment,profile,model,is_default,auto_index) VALUES($1,$2,$3,$4,$5,$6,$7)")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                .bind(&s.profile).bind(&s.model).bind(i64::from(s.is_default)).bind(i64::from(s.auto_index)).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM embedding_jobs WHERE organization=$1 AND project=$2 AND environment=$3 AND NOT EXISTS(SELECT 1 FROM project_embeddings p WHERE p.organization=embedding_jobs.organization AND p.project=embedding_jobs.project AND p.environment=embedding_jobs.environment AND p.profile=embedding_jobs.profile AND p.model=embedding_jobs.model AND p.auto_index=1)")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).execute(&mut *tx).await?;
        self.enqueue_embeddings(&mut tx, "").await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn enqueue_embeddings(
        &self,
        tx: &mut Transaction<'_, Any>,
        run_id: &str,
    ) -> Result<u64> {
        Ok(sqlx::query("INSERT INTO embedding_jobs(organization,project,environment,run_id,profile,model) SELECT r.organization,r.project,r.environment,r.id,p.profile,p.model FROM runs r JOIN project_embeddings p ON p.organization=r.organization AND p.project=r.project AND p.environment=r.environment WHERE r.organization=$1 AND r.project=$2 AND r.environment=$3 AND ($4='' OR r.id=$4) AND p.auto_index=1 ON CONFLICT DO NOTHING")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(run_id).execute(&mut **tx).await?.rows_affected())
    }

    /// Repair missed scheduling and explicitly retry failed/completed jobs when requested.
    pub async fn reindex_embeddings(&self, retry: bool) -> Result<u64> {
        let mut tx = self.transaction().await?;
        let mut count = self.enqueue_embeddings(&mut tx, "").await?;
        if retry {
            count += sqlx::query("UPDATE embedding_jobs SET status='pending',attempts=0,available_at=0,lease_token='' WHERE organization=$1 AND project=$2 AND environment=$3 AND status IN ('done','failed')")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).execute(&mut *tx).await?.rows_affected();
        }
        tx.commit().await?;
        Ok(count)
    }

    pub async fn embedding_job_counts(&self) -> Result<serde_json::Value> {
        let rows = sqlx::query("SELECT status,COUNT(*) AS count FROM embedding_jobs WHERE organization=$1 AND project=$2 AND environment=$3 GROUP BY status")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).fetch_all(&mut *self.connection().await?).await?;
        let mut counts = serde_json::json!({"pending":0,"done":0,"failed":0});
        for r in rows {
            counts[r.try_get::<String, _>("status")?] = r.try_get::<i64, _>("count")?.into();
        }
        Ok(counts)
    }

    /// Maintenance connection only: compare-and-swap leases fence workers across replicas.
    pub async fn claim_embedding_job(&self) -> Result<Option<EmbeddingJob>> {
        let now = Utc::now().timestamp();
        let rows = sqlx::query("SELECT organization,project,environment,run_id,profile,model,attempts,lease_token FROM embedding_jobs WHERE status='pending' AND available_at <= $1 ORDER BY available_at,run_id LIMIT 16")
            .bind(now).fetch_all(&mut *self.connection().await?).await?;
        for row in rows {
            let job = EmbeddingJob {
                scope: Scope {
                    organization: row.try_get("organization")?,
                    project: row.try_get("project")?,
                    environment: row.try_get("environment")?,
                },
                run_id: row.try_get("run_id")?,
                profile: row.try_get("profile")?,
                model: row.try_get("model")?,
                attempts: row.try_get("attempts")?,
                lease_token: refract_core::id("embedding"),
            };
            let changed = sqlx::query("UPDATE embedding_jobs SET lease_token=$1,available_at=$2 WHERE organization=$3 AND project=$4 AND environment=$5 AND run_id=$6 AND profile=$7 AND status='pending' AND available_at <= $8 AND lease_token=$9")
                .bind(&job.lease_token).bind(now+120).bind(&job.scope.organization).bind(&job.scope.project).bind(&job.scope.environment)
                .bind(&job.run_id).bind(&job.profile).bind(now).bind(row.try_get::<String,_>("lease_token")?).execute(&mut *self.connection().await?).await?.rows_affected();
            if changed == 1 {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    /// Fenced completion and vector insertion share a transaction. Retired jobs cannot publish.
    pub async fn finish_embedding_job(
        &self,
        job: &EmbeddingJob,
        values: Option<Vec<f64>>,
    ) -> Result<bool> {
        ensure!(self.scope == job.scope, "embedding job scope mismatch");
        let mut tx = self.transaction().await?;
        let status = if values.is_some() {
            "done"
        } else if job.attempts >= 9 {
            "failed"
        } else {
            "pending"
        };
        let changed = sqlx::query("UPDATE embedding_jobs SET status=$1,attempts=attempts+1,lease_token='',available_at=$2 WHERE organization=$3 AND project=$4 AND environment=$5 AND run_id=$6 AND profile=$7 AND model=$8 AND lease_token=$9")
            .bind(status).bind(Utc::now().timestamp() + (1i64 << job.attempts.min(9)) * 2)
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(&job.run_id).bind(&job.profile).bind(&job.model).bind(&job.lease_token).execute(&mut *tx).await?.rows_affected();
        if changed == 1
            && let Some(values) = values
        {
            self.put_embedding_tx(
                &mut tx,
                &job.run_id,
                &Embedding {
                    model: job.model.clone(),
                    values,
                },
            )
            .await?;
        }
        tx.commit().await?;
        Ok(changed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn embedding_queue_is_durable_scoped_and_fenced() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let settings = [EmbeddingSetting {
            profile: "test".into(),
            model: "test-v1".into(),
            is_default: true,
            auto_index: true,
        }];
        store.set_embedding_settings(&settings).await.unwrap();
        let run = Run::new("embedding queue");
        store.insert(&run).await.unwrap();
        assert!(!store.insert(&run).await.unwrap());
        let job = store.claim_embedding_job().await.unwrap().unwrap();
        assert!(store.claim_embedding_job().await.unwrap().is_none());
        assert!(
            store
                .finish_embedding_job(&job, Some(vec![1.0, 0.0]))
                .await
                .unwrap()
        );
        assert!(
            !store
                .finish_embedding_job(&job, Some(vec![0.0, 1.0]))
                .await
                .unwrap()
        );
        assert_eq!(store.embedding_job_counts().await.unwrap()["done"], 1);
        let other = store.scoped(Scope {
            project: "other".into(),
            ..Scope::default()
        });
        assert!(other.embedding_settings().await.unwrap().is_empty());
        assert!(other.finish_embedding_job(&job, None).await.is_err());
        store.reindex_embeddings(true).await.unwrap();
        let stale = store.claim_embedding_job().await.unwrap().unwrap();
        store.set_embedding_settings(&[]).await.unwrap();
        assert!(
            !store
                .finish_embedding_job(&stale, Some(vec![0.0, 1.0]))
                .await
                .unwrap()
        );
        store.set_embedding_settings(&settings).await.unwrap();
        let retained = store.claim_embedding_job().await.unwrap().unwrap();
        store
            .retain_since(Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert!(
            !store
                .finish_embedding_job(&retained, Some(vec![1.0, 1.0]))
                .await
                .unwrap()
        );
    }
}
