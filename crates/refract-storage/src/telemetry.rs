use super::*;
use futures_util::TryStreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub kind: String,
    #[serde(default)]
    pub trace_id: String,
    pub payload: Value,
}
impl Store {
    /// Redacted normalized records. Content IDs make whole-export retries idempotent.
    pub async fn insert_telemetry(&self, records: &[TelemetryRecord]) -> Result<u64> {
        ensure!(
            records.len() <= 10000,
            "at most 10000 telemetry records per export"
        );
        let mut tx = self.transaction().await?;
        let mut total = 0;
        let mut bytes = 0;
        let mut occurrences = std::collections::BTreeMap::<String, usize>::new();
        for record in records {
            ensure!(
                matches!(record.kind.as_str(), "logs" | "metrics"),
                "unsupported telemetry kind"
            );
            ensure!(
                record.trace_id.is_empty()
                    || (record.trace_id.len() == 32
                        && record.trace_id.bytes().all(|c| c.is_ascii_hexdigit())),
                "invalid telemetry trace id"
            );
            let serialized = serde_json::to_string(record)?;
            bytes += serialized.len();
            ensure!(bytes <= 16 * 1024 * 1024, "telemetry export exceeds 16 MiB");
            ensure!(
                serialized.len() <= 1024 * 1024,
                "telemetry record exceeds 1 MiB"
            );
            let hash = format!("{:x}", Sha256::digest(serialized.as_bytes()));
            let occurrence = occurrences.entry(hash.clone()).or_default();
            let id = format!("{hash}:{occurrence}");
            *occurrence += 1;
            let payload = match &self.options.encryption {
                Some(key) => key.seal(&self.scope, &format!("telemetry:{id}"), &serialized)?,
                None => serialized,
            };
            total+=sqlx::query("INSERT INTO telemetry(organization,project,environment,id,kind,trace_id,received_at,payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(id).bind(&record.kind).bind(&record.trace_id).bind(Utc::now().timestamp()).bind(payload).execute(&mut *tx).await?.rows_affected();
        }
        tx.commit().await?;
        Ok(total)
    }
    pub async fn telemetry(
        &self,
        kind: &str,
        trace_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TelemetryRecord>> {
        ensure!(
            matches!(kind, "logs" | "metrics")
                && (1..=100).contains(&limit)
                && (0..=1000000).contains(&offset),
            "invalid telemetry query"
        );
        ensure!(
            trace_id.is_empty()
                || (trace_id.len() == 32 && trace_id.bytes().all(|c| c.is_ascii_hexdigit())),
            "invalid trace filter"
        );
        let mut connection = self.connection().await?;
        let mut rows=sqlx::query("SELECT id,payload FROM telemetry WHERE organization=$1 AND project=$2 AND environment=$3 AND kind=$4 AND ($5='' OR trace_id=$5) ORDER BY received_at DESC,id DESC LIMIT $6 OFFSET $7")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(kind).bind(trace_id).bind(limit).bind(offset).fetch(&mut *connection);
        let mut records = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.try_next().await? {
            let id: String = row.try_get("id")?;
            let payload: String = row.try_get("payload")?;
            bytes += payload.len();
            if bytes > 12 * 1024 * 1024 {
                break;
            }
            let text = if payload.starts_with("enc:") {
                self.options
                    .encryption
                    .as_ref()
                    .ok_or_else(|| anyhow!("telemetry encryption key required"))?
                    .open(&self.scope, &format!("telemetry:{id}"), &payload)?
            } else {
                payload
            };
            records.push(serde_json::from_str(&text)?);
        }
        Ok(records)
    }
    pub async fn expire_telemetry(&self, before: DateTime<Utc>) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM telemetry WHERE organization=$1 AND project=$2 AND environment=$3 AND received_at<$4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(before.timestamp()).execute(&mut *self.connection().await?).await?.rows_affected())
    }
}
