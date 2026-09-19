use anyhow::Result;
use refract_core::Run;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::str::FromStr;
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}
impl Store {
    pub async fn open(url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(url)?.create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
    pub async fn ready(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
    /// Immutable run snapshots: duplicate IDs are conflicts, never silent overwrites.
    pub async fn insert(&self, run: &Run) -> Result<bool> {
        run.validate()?;
        let mut run = run.clone();
        run.redact();
        let result=sqlx::query("INSERT INTO runs(id,name,status,started_at,execution) VALUES(?,?,?,?,?) ON CONFLICT(id) DO NOTHING")
            .bind(&run.id).bind(&run.name).bind(serde_json::to_string(&run.status)?)
            .bind(run.started_at.to_rfc3339()).bind(serde_json::to_string(&run)?).execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }
    pub async fn get(&self, id: &str) -> Result<Option<Run>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT execution FROM runs WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| serde_json::from_str(&r.0).map_err(Into::into))
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<Run>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT execution FROM runs ORDER BY started_at DESC, id DESC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.0).map_err(Into::into))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn migrations_are_repeatable_and_preserve_data() {
        let path = std::env::temp_dir().join(refract_core::id("refract-db"));
        let url = format!("sqlite://{}", path.display());
        let first = Store::open(&url).await.unwrap();
        let run = Run::new("migration-test");
        assert!(first.insert(&run).await.unwrap());
        first.pool.close().await;
        let reopened = Store::open(&url).await.unwrap();
        assert_eq!(reopened.get(&run.id).await.unwrap().unwrap(), run);
        assert!(!reopened.insert(&run).await.unwrap());
        let versions: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&reopened.pool)
                .await
                .unwrap();
        assert_eq!(versions.0, 1);
        reopened.pool.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
