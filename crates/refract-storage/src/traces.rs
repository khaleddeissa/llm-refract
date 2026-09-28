use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TraceSpan {
    pub id: String,
    pub parent: Option<String>,
    pub event: refract_core::Event,
    pub ended_at: DateTime<Utc>,
}
impl Store {
    /// Transactionally merge distributed batches. Retries preserve immutable span contents.
    /// Callers must apply their redaction policy before persistence.
    pub async fn merge_trace(&self, trace_id: &str, spans: &[TraceSpan]) -> Result<Vec<TraceSpan>> {
        ensure!(
            trace_id.len() == 32 && trace_id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid trace id"
        );
        ensure!(
            !spans.is_empty() && spans.len() <= 1000,
            "trace batch must contain 1..1000 spans"
        );
        let mut tx = self.transaction().await?;
        // The upsert serializes concurrent writers of this trace on PostgreSQL and SQLite.
        sqlx::query("INSERT INTO trace_assemblies(organization,project,environment,trace_id,received_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT(organization,project,environment,trace_id) DO UPDATE SET received_at=excluded.received_at")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(trace_id).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM trace_assemblies WHERE organization=$1 AND project=$2 AND environment=$3")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).fetch_one(&mut *tx).await?;
        ensure!(count <= 10000, "tenant trace assembly capacity exceeded");
        for span in spans {
            let id = format!("trace:{trace_id}:{}", span.id);
            let value = serde_json::to_string(span)?;
            let encoded = match &self.options.encryption {
                Some(key) => key.seal(&self.scope, &id, &value)?,
                None => value.clone(),
            };
            let result = sqlx::query("INSERT INTO trace_spans(organization,project,environment,trace_id,span_id,payload) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(organization,project,environment,trace_id,span_id) DO NOTHING")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                .bind(trace_id).bind(&span.id).bind(encoded).execute(&mut *tx).await?;
            if result.rows_affected() == 0 {
                let (existing,): (String,) = sqlx::query_as("SELECT payload FROM trace_spans WHERE organization=$1 AND project=$2 AND environment=$3 AND trace_id=$4 AND span_id=$5")
                    .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                    .bind(trace_id).bind(&span.id).fetch_one(&mut *tx).await?;
                let decoded = self.trace_plaintext(&id, &existing)?;
                ensure!(
                    decoded == value,
                    "span id already contains different evidence"
                );
            }
        }
        let (count, size): (i64,i64) = sqlx::query_as("SELECT COUNT(*),CAST(COALESCE(SUM(LENGTH(payload)),0) AS BIGINT) FROM trace_spans WHERE organization=$1 AND project=$2 AND environment=$3 AND trace_id=$4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
            .bind(trace_id).fetch_one(&mut *tx).await?;
        ensure!(
            count <= 10000 && size <= 32 * 1024 * 1024,
            "trace assembly exceeds 10000 spans or 32 MiB"
        );
        let rows = sqlx::query("SELECT span_id,payload FROM trace_spans WHERE organization=$1 AND project=$2 AND environment=$3 AND trace_id=$4 ORDER BY span_id")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(trace_id).fetch_all(&mut *tx).await?;
        let mut result: Vec<TraceSpan> = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("span_id")?;
            let payload: String = row.try_get("payload")?;
            result.push(serde_json::from_str(
                &self.trace_plaintext(&format!("trace:{trace_id}:{id}"), &payload)?,
            )?);
        }
        // Reject cycles before committing join state, including cycles spanning different batches.
        let parents: std::collections::HashMap<_, _> = result
            .iter()
            .map(|span| (span.id.as_str(), span.parent.as_deref()))
            .collect();
        let mut complete = std::collections::HashSet::new();
        for span in &result {
            let mut path = std::collections::HashSet::new();
            let mut current = Some(span.id.as_str());
            while let Some(id) = current {
                if complete.contains(id) {
                    break;
                }
                ensure!(path.insert(id), "trace contains a parent cycle");
                current = parents.get(id).copied().flatten();
            }
            complete.extend(path);
        }
        tx.commit().await?;
        Ok(result)
    }
    fn trace_plaintext(&self, id: &str, payload: &str) -> Result<String> {
        if payload.starts_with("enc:") {
            self.options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("trace key unavailable"))?
                .open(&self.scope, id, payload)
        } else {
            Ok(payload.into())
        }
    }
    /// Assemblies are temporary join state. Published execution snapshots retain normal run policies.
    pub async fn expire_traces(&self, before: i64) -> Result<u64> {
        let mut tx = self.transaction().await?;
        sqlx::query("DELETE FROM trace_spans WHERE EXISTS(SELECT 1 FROM trace_assemblies a WHERE a.organization=trace_spans.organization AND a.project=trace_spans.project AND a.environment=trace_spans.environment AND a.trace_id=trace_spans.trace_id AND a.received_at < $1)")
            .bind(before).execute(&mut *tx).await?;
        let count = sqlx::query("DELETE FROM trace_assemblies WHERE received_at < $1")
            .bind(before)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(count)
    }
}
