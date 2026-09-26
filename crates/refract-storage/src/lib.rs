mod encryption;
pub use encryption::Encryption;

use anyhow::{Result, anyhow, ensure};
use chrono::{DateTime, Utc};
use refract_core::Run;
use serde::{Deserialize, Serialize};
use sqlx::{Any, AnyPool, Row, Transaction, any::AnyPoolOptions, migrate::MigrateDatabase};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub organization: String,
    pub project: String,
    pub environment: String,
}
impl Default for Scope {
    fn default() -> Self {
        Self {
            organization: "local".into(),
            project: "default".into(),
            environment: "development".into(),
        }
    }
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.organization, &self.project, &self.environment] {
            ensure!(
                !value.is_empty()
                    && value.len() <= 100
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                "scope components must contain 1..100 ASCII letters, digits, hyphens or underscores"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct StoreOptions {
    pub encryption: Option<Encryption>,
    /// Each target receives an independently acknowledged durable outbox entry.
    pub outbox_targets: Vec<String>,
}
#[derive(Clone)]
pub struct Store {
    pool: AnyPool,
    scope: Scope,
    options: StoreOptions,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunFilter {
    pub q: String,
    pub status: String,
    pub model: String,
    pub tool: String,
    pub min_duration_ms: Option<f64>,
    pub min_event_duration_ms: Option<f64>,
    pub min_cost_usd: Option<f64>,
    pub after: String,
    pub before: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
impl RunFilter {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.q.len() <= 500 && self.model.len() <= 500 && self.tool.len() <= 500,
            "search text is too long"
        );
        ensure!(
            ["", "running", "completed", "failed"].contains(&self.status.as_str()),
            "invalid run status"
        );
        ensure!(
            (1..=1000).contains(&self.limit.unwrap_or(100)),
            "limit must be between 1 and 1000"
        );
        ensure!(self.offset.unwrap_or(0) >= 0, "offset must be nonnegative");
        for value in [
            self.min_duration_ms,
            self.min_event_duration_ms,
            self.min_cost_usd,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                value.is_finite() && value >= 0.0,
                "minimum thresholds must be finite and nonnegative"
            );
        }
        for value in [&self.after, &self.before] {
            if !value.is_empty() {
                DateTime::parse_from_rfc3339(value)?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct SearchPage {
    pub runs: Vec<Run>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
#[derive(Debug, Serialize)]
pub struct BatchReceipt {
    pub accepted: usize,
    pub duplicates: usize,
    pub run_ids: Vec<String>,
}
#[derive(Debug)]
pub struct SnapshotConflict(pub String);
impl std::fmt::Display for SnapshotConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run id {} already exists with different content", self.0)
    }
}
impl std::error::Error for SnapshotConflict {}
#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub actor: String,
    pub action: String,
    pub resource: String,
    pub status: i64,
    pub timestamp: String,
}
#[derive(Debug, Serialize)]
pub struct OutboxJob {
    pub id: String,
    pub scope: Scope,
    pub run_id: String,
    pub target: String,
    pub operation: String,
    pub attempts: i64,
}

const FILTER: &str = " FROM runs r WHERE r.organization=$1 AND r.project=$2 AND r.environment=$3
 AND ($4='' OR LOWER(r.name) LIKE $4 ESCAPE '\\' OR EXISTS(SELECT 1 FROM run_events e WHERE e.organization=r.organization AND e.project=r.project AND e.environment=r.environment AND e.run_id=r.id AND LOWER(e.name) LIKE $4 ESCAPE '\\'))
 AND ($5='' OR r.status=$5)
 AND ($6='' OR EXISTS(SELECT 1 FROM run_events e WHERE e.organization=r.organization AND e.project=r.project AND e.environment=r.environment AND e.run_id=r.id AND e.model=$6))
 AND (($7='' AND $9<0) OR EXISTS(SELECT 1 FROM run_events e WHERE e.organization=r.organization AND e.project=r.project AND e.environment=r.environment AND e.run_id=r.id AND ($7='' OR (e.kind='tool.call' AND e.name=$7)) AND e.duration_ms >= $9))
 AND r.duration_ms >= $8 AND r.cost_usd >= $10 AND ($11='' OR r.started_at >= $11) AND ($12='' OR r.started_at < $12)";

impl Store {
    pub async fn open(url: &str) -> Result<Self> {
        Self::open_with_options(url, StoreOptions::default()).await
    }
    pub async fn open_with_options(url: &str, options: StoreOptions) -> Result<Self> {
        ensure!(
            url.starts_with("sqlite:")
                || url.starts_with("postgres:")
                || url.starts_with("postgresql:"),
            "supported databases are SQLite and PostgreSQL"
        );
        sqlx::any::install_default_drivers();
        if url.starts_with("sqlite:") && !Any::database_exists(url).await? {
            Any::create_database(url).await?;
        }
        let pool = AnyPoolOptions::new()
            .max_connections(if url.starts_with("sqlite:") { 1 } else { 10 })
            .connect(url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let store = Self {
            pool,
            scope: Scope::default(),
            options,
        };
        // Detect lost/wrong keys during startup rather than reporting healthy then failing reads.
        let encrypted = sqlx::query("SELECT organization,project,environment,id,execution FROM runs WHERE execution LIKE 'enc:%' LIMIT 1")
            .fetch_optional(&store.pool).await?;
        if let Some(row) = encrypted {
            let scoped = store.scoped(Scope {
                organization: row.try_get("organization")?,
                project: row.try_get("project")?,
                environment: row.try_get("environment")?,
            });
            scoped.decode(row.try_get("id")?, row.try_get("execution")?)?;
        }
        store.backfill().await?;
        Ok(store)
    }
    pub fn scoped(&self, scope: Scope) -> Self {
        Self {
            scope,
            ..self.clone()
        }
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub async fn ready(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
    fn encode(&self, run: &Run) -> Result<String> {
        let text = serde_json::to_string(run)?;
        match &self.options.encryption {
            Some(key) => key.seal(&self.scope, &run.id, &text),
            None => Ok(text),
        }
    }
    fn decode(&self, id: &str, text: &str) -> Result<Run> {
        let plaintext = if text.starts_with("enc:") {
            self.options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("encrypted database requires REFRACT_ENCRYPTION_KEY"))?
                .open(&self.scope, id, text)?
        } else {
            text.to_owned()
        };
        Ok(serde_json::from_str(&plaintext)?)
    }
    async fn backfill(&self) -> Result<()> {
        let rows = sqlx::query("SELECT organization,project,environment,id,execution,indexed FROM runs WHERE indexed=0 OR ($1=1 AND execution NOT LIKE 'enc:v1:%')")
            .bind(i64::from(self.options.encryption.is_some())).fetch_all(&self.pool).await?;
        for row in rows {
            let scoped = self.scoped(Scope {
                organization: row.try_get("organization")?,
                project: row.try_get("project")?,
                environment: row.try_get("environment")?,
            });
            let id: String = row.try_get("id")?;
            let mut run = scoped.decode(&id, row.try_get("execution")?)?;
            run.validate()?;
            run.redact();
            let mut tx = self.pool.begin().await?;
            if row.try_get::<i64, _>("indexed")? == 0 {
                scoped.index_events(&mut tx, &run).await?;
            }
            sqlx::query("UPDATE runs SET execution=$1,duration_ms=$2,cost_usd=$3,indexed=1 WHERE organization=$4 AND project=$5 AND environment=$6 AND id=$7")
                .bind(scoped.encode(&run)?).bind(duration_ms(&run)).bind(cost_usd(&run))
                .bind(&scoped.scope.organization).bind(&scoped.scope.project).bind(&scoped.scope.environment).bind(&id).execute(&mut *tx).await?;
            tx.commit().await?;
        }
        Ok(())
    }
    /// Immutable snapshots. The same ID may independently exist in different authenticated scopes.
    pub async fn insert(&self, run: &Run) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let inserted = self.insert_tx(&mut tx, run).await?;
        tx.commit().await?;
        Ok(inserted)
    }
    /// Atomic batch with content-checked idempotency, including duplicates within a batch.
    pub async fn insert_batch(&self, runs: &[Run]) -> Result<BatchReceipt> {
        let mut tx = self.pool.begin().await?;
        let mut accepted = 0;
        for run in runs {
            if self.insert_tx(&mut tx, run).await? {
                accepted += 1;
            } else {
                let (payload,): (String,) = sqlx::query_as("SELECT execution FROM runs WHERE organization=$1 AND project=$2 AND environment=$3 AND id=$4")
                    .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&run.id).fetch_one(&mut *tx).await?;
                let mut normalized = run.clone();
                normalized.redact();
                if self.decode(&run.id, &payload)? != normalized {
                    return Err(SnapshotConflict(run.id.clone()).into());
                }
            }
        }
        tx.commit().await?;
        Ok(BatchReceipt {
            accepted,
            duplicates: runs.len() - accepted,
            run_ids: runs.iter().map(|run| run.id.clone()).collect(),
        })
    }
    async fn insert_tx(&self, tx: &mut Transaction<'_, Any>, run: &Run) -> Result<bool> {
        run.validate()?;
        let mut run = run.clone();
        run.redact();
        let status = serde_json::to_value(&run.status)?
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let result = sqlx::query("INSERT INTO runs(organization,project,environment,id,name,status,started_at,received_at,duration_ms,cost_usd,execution,indexed) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1) ON CONFLICT(organization,project,environment,id) DO NOTHING")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&run.id)
            .bind(&run.name).bind(status).bind(run.started_at.to_rfc3339()).bind(Utc::now().to_rfc3339())
            .bind(duration_ms(&run)).bind(cost_usd(&run)).bind(self.encode(&run)?).execute(&mut **tx).await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        self.index_events(tx, &run).await?;
        for target in &self.options.outbox_targets {
            self.enqueue(tx, &run.id, target, "put").await?;
        }
        Ok(true)
    }
    async fn index_events(&self, tx: &mut Transaction<'_, Any>, run: &Run) -> Result<()> {
        for e in &run.events {
            sqlx::query("INSERT INTO run_events(organization,project,environment,run_id,event_id,kind,name,model,duration_ms) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&run.id).bind(&e.id)
                .bind(serde_json::to_value(&e.kind)?.as_str().unwrap_or_default()).bind(&e.name)
                .bind(e.attributes.get("model").and_then(|v| v.as_str()).unwrap_or_default()).bind(e.duration_ms).execute(&mut **tx).await?;
        }
        Ok(())
    }
    async fn enqueue(
        &self,
        tx: &mut Transaction<'_, Any>,
        run_id: &str,
        target: &str,
        operation: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO outbox(id,organization,project,environment,run_id,target,operation) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
            .bind(refract_core::id("delivery")).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(run_id).bind(target).bind(operation).execute(&mut **tx).await?;
        Ok(())
    }
    pub async fn get(&self, id: &str) -> Result<Option<Run>> {
        self.stored_payload(id)
            .await?
            .map(|payload| self.decode(id, &payload))
            .transpose()
    }
    /// Preserve the stored envelope for delivery. Payloads are encrypted when encryption is configured.
    pub async fn stored_payload(&self, id: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT execution FROM runs WHERE organization=$1 AND project=$2 AND environment=$3 AND id=$4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.0))
    }
    pub async fn list(&self) -> Result<Vec<Run>> {
        Ok(self.search(&RunFilter::default()).await?.runs)
    }
    pub async fn search(&self, filter: &RunFilter) -> Result<SearchPage> {
        filter.validate()?;
        let text = if filter.q.is_empty() {
            String::new()
        } else {
            format!(
                "%{}%",
                filter
                    .q
                    .to_lowercase()
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        };
        let after = canonical_time(&filter.after)?;
        let before = canonical_time(&filter.before)?;
        let select = format!(
            "SELECT r.id,r.execution{FILTER} ORDER BY r.started_at DESC,r.id DESC LIMIT $13 OFFSET $14"
        );
        let count = format!("SELECT COUNT(*){FILTER}");
        macro_rules! bind_filter {
            ($query:expr) => {
                $query
                    .bind(&self.scope.organization)
                    .bind(&self.scope.project)
                    .bind(&self.scope.environment)
                    .bind(&text)
                    .bind(&filter.status)
                    .bind(&filter.model)
                    .bind(&filter.tool)
                    .bind(filter.min_duration_ms.unwrap_or(-1.0))
                    .bind(filter.min_event_duration_ms.unwrap_or(-1.0))
                    .bind(filter.min_cost_usd.unwrap_or(-1.0))
                    .bind(&after)
                    .bind(&before)
            };
        }
        let limit = filter.limit.unwrap_or(100);
        let offset = filter.offset.unwrap_or(0);
        let rows: Vec<(String, String)> = bind_filter!(sqlx::query_as(&select))
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        let (total,): (i64,) = bind_filter!(sqlx::query_as(&count))
            .fetch_one(&self.pool)
            .await?;
        Ok(SearchPage {
            runs: rows
                .into_iter()
                .map(|(id, payload)| self.decode(&id, &payload))
                .collect::<Result<_>>()?,
            total,
            limit,
            offset,
        })
    }
    pub async fn audit(
        &self,
        actor: &str,
        action: &str,
        resource: &str,
        status: u16,
    ) -> Result<()> {
        sqlx::query("INSERT INTO audit_log(id,organization,project,environment,actor,action,resource,status,timestamp) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(refract_core::id("audit")).bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(actor).bind(action).bind(resource).bind(i64::from(status)).bind(Utc::now().to_rfc3339()).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn audit_log(&self, limit: i64, offset: i64) -> Result<Vec<AuditEntry>> {
        ensure!(
            (1..=1000).contains(&limit) && offset >= 0,
            "invalid audit pagination"
        );
        let rows = sqlx::query("SELECT id,actor,action,resource,status,timestamp FROM audit_log WHERE organization=$1 AND project=$2 AND environment=$3 ORDER BY timestamp DESC,id DESC LIMIT $4 OFFSET $5")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(limit).bind(offset).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                Ok(AuditEntry {
                    id: r.try_get("id")?,
                    actor: r.try_get("actor")?,
                    action: r.try_get("action")?,
                    resource: r.try_get("resource")?,
                    status: r.try_get("status")?,
                    timestamp: r.try_get("timestamp")?,
                })
            })
            .collect()
    }
    /// Delete scoped snapshots using server receipt time, never attacker-controlled event time.
    /// Object deletion is queued transactionally; audit history is retained separately.
    pub async fn retain_since(&self, before: DateTime<Utc>) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM runs WHERE organization=$1 AND project=$2 AND environment=$3 AND received_at < $4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(before.to_rfc3339()).fetch_all(&mut *tx).await?;
        for (id,) in &ids {
            sqlx::query("DELETE FROM outbox WHERE organization=$1 AND project=$2 AND environment=$3 AND run_id=$4")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(id).execute(&mut *tx).await?;
            for target in &self.options.outbox_targets {
                self.enqueue(&mut tx, id, target, "delete").await?;
            }
            // Explicit event cleanup also works for SQLite connections with foreign_keys disabled.
            sqlx::query("DELETE FROM run_events WHERE organization=$1 AND project=$2 AND environment=$3 AND run_id=$4")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM runs WHERE organization=$1 AND project=$2 AND environment=$3 AND id=$4")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(ids.len() as u64)
    }
    /// Durable at-least-once delivery with a 120-second timeout lease. Consumers must deduplicate IDs.
    pub async fn claim_outbox(&self) -> Result<Option<OutboxJob>> {
        let now = Utc::now().timestamp();
        let rows = sqlx::query("SELECT id,organization,project,environment,run_id,target,operation,attempts,available_at FROM outbox WHERE available_at <= $1 ORDER BY available_at,id LIMIT 10")
            .bind(now).fetch_all(&self.pool).await?;
        for r in rows {
            let id: String = r.try_get("id")?;
            let claimed =
                sqlx::query("UPDATE outbox SET available_at=$1 WHERE id=$2 AND available_at=$3")
                    .bind(now + 120)
                    .bind(&id)
                    .bind(r.try_get::<i64, _>("available_at")?)
                    .execute(&self.pool)
                    .await?;
            if claimed.rows_affected() == 1 {
                return Ok(Some(OutboxJob {
                    id,
                    scope: Scope {
                        organization: r.try_get("organization")?,
                        project: r.try_get("project")?,
                        environment: r.try_get("environment")?,
                    },
                    run_id: r.try_get("run_id")?,
                    target: r.try_get("target")?,
                    operation: r.try_get("operation")?,
                    attempts: r.try_get("attempts")?,
                }));
            }
        }
        Ok(None)
    }
    pub async fn acknowledge(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM outbox WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn retry(&self, job: &OutboxJob) -> Result<()> {
        let delay = (1_i64 << job.attempts.saturating_add(1).clamp(0, 12)).min(3600);
        sqlx::query("UPDATE outbox SET attempts=attempts+1,available_at=$1 WHERE id=$2")
            .bind(Utc::now().timestamp() + delay)
            .bind(&job.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn scopes(&self) -> Result<Vec<Scope>> {
        let rows = sqlx::query("SELECT DISTINCT organization,project,environment FROM runs")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Scope {
                    organization: r.try_get("organization")?,
                    project: r.try_get("project")?,
                    environment: r.try_get("environment")?,
                })
            })
            .collect()
    }
    pub async fn outbox_pending(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM outbox WHERE organization=$1 AND project=$2 AND environment=$3",
        )
        .bind(&self.scope.organization)
        .bind(&self.scope.project)
        .bind(&self.scope.environment)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }
}
fn canonical_time(value: &str) -> Result<String> {
    if value.is_empty() {
        Ok(String::new())
    } else {
        Ok(DateTime::parse_from_rfc3339(value)?
            .with_timezone(&Utc)
            .to_rfc3339())
    }
}
fn duration_ms(run: &Run) -> f64 {
    run.ended_at
        .map(|end| (end - run.started_at).num_microseconds().unwrap_or(0) as f64 / 1000.0)
        .unwrap_or_else(|| run.events.iter().map(|e| e.duration_ms).fold(0.0, f64::max))
}
fn cost_usd(run: &Run) -> f64 {
    run.events
        .iter()
        .filter_map(|e| e.attributes.get("cost_usd").and_then(|v| v.as_f64()))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    fn fixture() -> Run {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap()
    }
    #[tokio::test]
    async fn migrations_preserve_data_and_scopes_isolate_ids() {
        let path = std::env::temp_dir().join(refract_core::id("refract-db"));
        let url = format!("sqlite://{}", path.display());
        let first = Store::open(&url).await.unwrap();
        let run = fixture();
        assert!(first.insert(&run).await.unwrap());
        first.pool.close().await;
        let store = Store::open(&url).await.unwrap();
        assert_eq!(store.get(&run.id).await.unwrap().unwrap(), run);
        assert!(!store.insert(&run).await.unwrap());
        let other = store.scoped(Scope {
            project: "other".into(),
            ..Scope::default()
        });
        assert!(other.get(&run.id).await.unwrap().is_none());
        assert!(other.insert(&run).await.unwrap());
        let (versions,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations WHERE success=TRUE")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(versions, 2);
        store.pool.close().await;
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn batches_are_idempotent_and_conflicts_roll_back() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let run = fixture();
        assert_eq!(
            store
                .insert_batch(&[run.clone(), run.clone()])
                .await
                .unwrap()
                .accepted,
            1
        );
        assert_eq!(
            store
                .insert_batch(std::slice::from_ref(&run))
                .await
                .unwrap()
                .duplicates,
            1
        );
        let first = Run::new("new");
        let mut conflict = run;
        conflict.name = "different".into();
        assert!(
            store
                .insert_batch(&[first.clone(), conflict])
                .await
                .unwrap_err()
                .downcast_ref::<SnapshotConflict>()
                .is_some()
        );
        assert!(store.get(&first.id).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn search_is_filtered_paginated_and_parameterized() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let mut run = fixture();
        run.events[0].attributes["model"] = "test-model".into();
        run.events[0].attributes["cost_usd"] = 0.25.into();
        store.insert(&run).await.unwrap();
        store.insert(&Run::new("another")).await.unwrap();
        let page = store
            .search(&RunFilter {
                model: "test-model".into(),
                min_cost_usd: Some(0.2),
                limit: Some(1),
                ..RunFilter::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.runs[0].id, run.id);
        assert_eq!(
            store
                .search(&RunFilter {
                    q: "%' OR 1=1 --".into(),
                    ..RunFilter::default()
                })
                .await
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            store
                .search(&RunFilter {
                    offset: Some(1),
                    limit: Some(1),
                    ..RunFilter::default()
                })
                .await
                .unwrap()
                .runs
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn encryption_authenticates_scope_and_outbox_is_durable() {
        let key = Encryption::from_base64(&STANDARD.encode([7u8; 32])).unwrap();
        let store = Store::open_with_options(
            "sqlite::memory:",
            StoreOptions {
                encryption: Some(key.clone()),
                outbox_targets: vec!["s3".into()],
            },
        )
        .await
        .unwrap();
        let run = fixture();
        store.insert(&run).await.unwrap();
        let encoded = store.stored_payload(&run.id).await.unwrap().unwrap();
        assert!(encoded.starts_with("enc:v1:"));
        assert!(!encoded.contains(&run.name));
        assert_eq!(store.get(&run.id).await.unwrap().unwrap(), run);
        assert!(
            key.open(
                &Scope {
                    project: "other".into(),
                    ..Scope::default()
                },
                &run.id,
                &encoded
            )
            .is_err()
        );
        let job = store.claim_outbox().await.unwrap().unwrap();
        assert!(store.claim_outbox().await.unwrap().is_none());
        store.acknowledge(&job.id).await.unwrap();
        assert_eq!(store.outbox_pending().await.unwrap(), 0);
        assert_eq!(
            store
                .retain_since(Utc::now() + chrono::Duration::seconds(1))
                .await
                .unwrap(),
            1
        );
        assert!(store.get(&run.id).await.unwrap().is_none());
        assert_eq!(
            store.claim_outbox().await.unwrap().unwrap().operation,
            "delete"
        );
    }
}

#[cfg(test)]
mod production_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[tokio::test]
    async fn encrypted_reopen_rejects_wrong_or_missing_key_and_tampering() {
        let path = std::env::temp_dir().join(refract_core::id("encrypted-db"));
        let url = format!("sqlite://{}", path.display());
        let key = Encryption::from_base64(&STANDARD.encode([7u8; 32])).unwrap();
        let options = StoreOptions {
            encryption: Some(key.clone()),
            ..Default::default()
        };
        let store = Store::open_with_options(&url, options.clone())
            .await
            .unwrap();
        let run = Run::new("private-data");
        store.insert(&run).await.unwrap();
        let value = store.stored_payload(&run.id).await.unwrap().unwrap();
        let mut bytes = STANDARD
            .decode(value.strip_prefix("enc:v1:").unwrap())
            .unwrap();
        bytes[12] ^= 1;
        assert!(
            key.open(
                &Scope::default(),
                &run.id,
                &format!("enc:v1:{}", STANDARD.encode(bytes))
            )
            .is_err()
        );
        assert!(key.open(&Scope::default(), "different-id", &value).is_err());
        store.pool.close().await;
        assert!(Store::open(&url).await.is_err());
        assert!(
            Store::open_with_options(
                &url,
                StoreOptions {
                    encryption: Some(Encryption::from_base64(&STANDARD.encode([8u8; 32])).unwrap()),
                    ..Default::default()
                }
            )
            .await
            .is_err()
        );
        let reopened = Store::open_with_options(&url, options).await.unwrap();
        assert_eq!(reopened.get(&run.id).await.unwrap(), Some(run));
        reopened.pool.close().await;
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL database; CI supplies the service"]
    async fn postgres_platform_contract() {
        let url = std::env::var("REFRACT_TEST_POSTGRES_URL")
            .expect("set REFRACT_TEST_POSTGRES_URL to a disposable PostgreSQL database");
        let store = Store::open_with_options(
            &url,
            StoreOptions {
                encryption: Some(Encryption::from_base64(&STANDARD.encode([9u8; 32])).unwrap()),
                outbox_targets: vec!["webhook".into()],
            },
        )
        .await
        .unwrap()
        .scoped(Scope {
            project: refract_core::id("pgtest"),
            ..Scope::default()
        });
        let mut run = Run::new("Postgres integration");
        run.name = "100% scoped".into();
        assert_eq!(
            store
                .insert_batch(&[run.clone(), run.clone()])
                .await
                .unwrap()
                .accepted,
            1
        );
        assert_eq!(store.get(&run.id).await.unwrap(), Some(run.clone()));
        assert_eq!(
            store
                .search(&RunFilter {
                    q: "100%".into(),
                    ..Default::default()
                })
                .await
                .unwrap()
                .total,
            1
        );
        let other = store.scoped(Scope {
            project: "not-the-test-project".into(),
            ..Scope::default()
        });
        assert!(other.get(&run.id).await.unwrap().is_none());
        store.audit("test", "POST", "/v1/runs", 201).await.unwrap();
        assert_eq!(store.audit_log(10, 0).await.unwrap().len(), 1);
        assert_eq!(store.outbox_pending().await.unwrap(), 1);
        assert_eq!(
            store
                .retain_since(Utc::now() + chrono::Duration::seconds(1))
                .await
                .unwrap(),
            1
        );
        assert!(store.get(&run.id).await.unwrap().is_none());
        assert_eq!(store.outbox_pending().await.unwrap(), 1);
    }
}
