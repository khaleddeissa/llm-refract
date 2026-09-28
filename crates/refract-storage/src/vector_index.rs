use super::*;
use futures_util::TryStreamExt;
use instant_distance::{Builder, HnswMap, Point, Search};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::{Mutex, Semaphore};

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchMode {
    #[default]
    Auto,
    Exact,
    Approximate,
}

#[derive(Clone, PartialEq, Eq)]
struct IndexKey {
    scope: Scope,
    model: String,
    dimensions: usize,
}
#[derive(Clone)]
struct UnitVector(Vec<f32>);
impl Point for UnitVector {
    fn distance(&self, other: &Self) -> f32 {
        // Euclidean distance on unit vectors is a metric with the same cosine ordering.
        self.0
            .iter()
            .zip(&other.0)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>()
            .sqrt()
    }
}
struct CachedIndex {
    key: IndexKey,
    generation: i64,
    estimated_bytes: usize,
    graph: HnswMap<UnitVector, String>,
}
pub(crate) struct VectorCache {
    entries: Mutex<VecDeque<Arc<CachedIndex>>>,
    build: Semaphore,
    budget: usize,
}
impl VectorCache {
    pub(crate) fn from_env() -> Result<Self> {
        let mb = std::env::var("REFRACT_VECTOR_CACHE_MB")
            .unwrap_or_else(|_| "256".into())
            .parse::<usize>()?;
        ensure!(
            (16..=32768).contains(&mb),
            "REFRACT_VECTOR_CACHE_MB must be 16..32768"
        );
        Ok(Self {
            entries: Mutex::default(),
            build: Semaphore::new(1),
            budget: mb * 1024 * 1024,
        })
    }
    async fn get(&self, key: &IndexKey, generation: i64) -> Option<Arc<CachedIndex>> {
        let mut entries = self.entries.lock().await;
        let index = entries
            .iter()
            .position(|item| &item.key == key && item.generation == generation)?;
        let item = entries.remove(index)?;
        entries.push_back(item.clone());
        Some(item)
    }
    async fn insert(&self, index: Arc<CachedIndex>) {
        let mut entries = self.entries.lock().await;
        entries.retain(|entry| entry.key != index.key);
        let mut bytes = entries.iter().map(|e| e.estimated_bytes).sum::<usize>();
        while bytes + index.estimated_bytes > self.budget {
            let Some(old) = entries.pop_front() else {
                break;
            };
            bytes -= old.estimated_bytes;
        }
        entries.push_back(index);
    }
}

impl Store {
    pub(crate) async fn bump_vector_generation(
        &self,
        tx: &mut Transaction<'_, Any>,
        model: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO vector_generations(organization,project,environment,model,generation) VALUES($1,$2,$3,$4,1) ON CONFLICT(organization,project,environment,model) DO UPDATE SET generation=vector_generations.generation+1")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(model).execute(&mut **tx).await?;
        Ok(())
    }
    async fn vector_generation(&self, model: &str) -> Result<i64> {
        let row:Option<(i64,)>=sqlx::query_as("SELECT generation FROM vector_generations WHERE organization=$1 AND project=$2 AND environment=$3 AND model=$4")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(model).fetch_optional(&mut *self.connection().await?).await?;
        Ok(row.map_or(0, |row| row.0))
    }
    fn decode_vector(&self, run_id: &str, model: &str, payload: String) -> Result<Vec<f64>> {
        let decoded = if payload.starts_with("enc:") {
            self.options
                .encryption
                .as_ref()
                .ok_or_else(|| anyhow!("embedding key is unavailable"))?
                .open(
                    &self.scope,
                    &format!("embedding:{run_id}:{model}"),
                    &payload,
                )?
        } else {
            payload
        };
        Ok(serde_json::from_str(&decoded)?)
    }
    /// Auto uses HNSW for larger namespaces and exact search for small or uncached oversized sets.
    pub async fn vector_search(&self, query: &Embedding, limit: usize) -> Result<Vec<VectorMatch>> {
        self.vector_search_with_mode(query, limit, VectorSearchMode::Auto)
            .await
    }
    pub async fn vector_search_with_mode(
        &self,
        query: &Embedding,
        limit: usize,
        mode: VectorSearchMode,
    ) -> Result<Vec<VectorMatch>> {
        query.validate()?;
        ensure!(
            (1..=100).contains(&limit),
            "vector result limit must be 1..100"
        );
        if matches!(mode, VectorSearchMode::Exact) {
            return self.exact_vectors(query, limit).await;
        }
        let (count,):(i64,)=sqlx::query_as("SELECT COUNT(*) FROM run_embeddings WHERE organization=$1 AND project=$2 AND environment=$3 AND model=$4 AND dimensions=$5")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&query.model).bind(query.values.len() as i64).fetch_one(&mut *self.connection().await?).await?;
        // Conservative allowance includes graph connections, IDs, f32 vectors, and build copies.
        let per_vector = query.values.len() * 16 + 2048;
        if count == 0
            || (count < 256 && matches!(mode, VectorSearchMode::Auto))
            || count as usize > self.vector_cache.budget / per_vector
        {
            return self.exact_vectors(query, limit).await;
        }
        let key = IndexKey {
            scope: self.scope.clone(),
            model: query.model.clone(),
            dimensions: query.values.len(),
        };
        let generation = self.vector_generation(&query.model).await?;
        let cached = if let Some(index) = self.vector_cache.get(&key, generation).await {
            index
        } else {
            let _permit = self.vector_cache.build.acquire().await?;
            // Another request may have filled this namespace while we waited.
            let generation = self.vector_generation(&query.model).await?;
            if let Some(index) = self.vector_cache.get(&key, generation).await {
                index
            } else {
                let mut connection = self.connection().await?;
                let mut rows=sqlx::query("SELECT run_id,embedding FROM run_embeddings WHERE organization=$1 AND project=$2 AND environment=$3 AND model=$4 AND dimensions=$5 ORDER BY run_id")
                    .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&query.model).bind(query.values.len() as i64).fetch(&mut *connection);
                let mut vectors = Vec::new();
                let mut ids = Vec::new();
                let mut too_large = false;
                while let Some(row) = rows.try_next().await? {
                    if vectors.len() >= self.vector_cache.budget / per_vector {
                        too_large = true;
                        break;
                    }
                    let id: String = row.try_get("run_id")?;
                    let values =
                        self.decode_vector(&id, &query.model, row.try_get("embedding")?)?;
                    ensure!(
                        values.len() == key.dimensions,
                        "stored embedding dimension mismatch"
                    );
                    vectors.push(UnitVector(
                        values.into_iter().map(|v| v as f32).collect::<Vec<_>>(),
                    ));
                    ids.push(id);
                }
                drop(rows);
                drop(connection);
                if too_large || self.vector_generation(&query.model).await? != generation {
                    return self.exact_vectors(query, limit).await;
                }
                let index = tokio::task::spawn_blocking(move || {
                    let estimated_bytes = vectors.len() * per_vector;
                    let graph = Builder::default()
                        .ef_construction(160)
                        .ef_search(400)
                        .seed(42)
                        .build(vectors, ids);
                    Arc::new(CachedIndex {
                        key,
                        generation,
                        estimated_bytes,
                        graph,
                    })
                })
                .await?;
                self.vector_cache.insert(index.clone()).await;
                index
            }
        };
        let q = UnitVector(query.normalized().into_iter().map(|v| v as f32).collect());
        let index = cached.clone();
        let found = tokio::task::spawn_blocking(move || {
            let mut search = Search::default();
            index
                .graph
                .search(&q, &mut search)
                .take(limit)
                .map(|n| VectorMatch {
                    run_id: n.value.clone(),
                    score: (1.0 - f64::from(n.distance).powi(2) / 2.0).clamp(-1.0, 1.0),
                })
                .collect::<Vec<_>>()
        })
        .await?;
        // Do not serve an index made stale by a concurrent vector replacement or retention pass.
        if self.vector_generation(&query.model).await? != cached.generation {
            return self.exact_vectors(query, limit).await;
        }
        let mut found = found;
        sort_matches(&mut found);
        Ok(found)
    }
    /// A single streaming SQL statement has a consistent snapshot and no candidate-count cutoff.
    async fn exact_vectors(&self, query: &Embedding, limit: usize) -> Result<Vec<VectorMatch>> {
        let normalized = query.normalized();
        let mut connection = self.connection().await?;
        let mut rows=sqlx::query("SELECT run_id,embedding FROM run_embeddings WHERE organization=$1 AND project=$2 AND environment=$3 AND model=$4 AND dimensions=$5 ORDER BY run_id")
            .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment).bind(&query.model).bind(query.values.len() as i64).fetch(&mut *connection);
        let mut matches = Vec::with_capacity(limit + 1);
        let mut visited = 0;
        while let Some(row) = rows.try_next().await? {
            let run_id: String = row.try_get("run_id")?;
            let values = self.decode_vector(&run_id, &query.model, row.try_get("embedding")?)?;
            ensure!(
                values.len() == normalized.len(),
                "stored embedding dimension mismatch"
            );
            let score = values
                .iter()
                .zip(&normalized)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                .clamp(-1.0, 1.0);
            let found = VectorMatch { run_id, score };
            let index = matches.partition_point(|m: &VectorMatch| {
                m.score > found.score || (m.score == found.score && m.run_id < found.run_id)
            });
            if index < limit {
                matches.insert(index, found);
                matches.truncate(limit);
            }
            visited += 1;
            if visited % 256 == 0 {
                tokio::task::yield_now().await;
            }
        }
        Ok(matches)
    }
}
fn sort_matches(matches: &mut [VectorMatch]) {
    matches.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exact_and_approximate_search_cross_the_old_candidate_limit() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let mut tx = store.transaction().await.unwrap();
        let mut target = vec![];
        for index in 0..10050u64 {
            let mut run = Run::new("vector capacity");
            run.id = format!("v{index:05}");
            store.insert_tx(&mut tx, &run).await.unwrap();
            let mut seed = index + 1;
            let values: Vec<f64> = (0..8)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((seed >> 32) as f64 / u32::MAX as f64) * 2.0 - 1.0
                })
                .collect();
            store
                .put_embedding_tx(
                    &mut tx,
                    &run.id,
                    &Embedding {
                        model: "large".into(),
                        values: values.clone(),
                    },
                )
                .await
                .unwrap();
            if index == 10049 {
                target = values;
            }
        }
        tx.commit().await.unwrap();
        let query = Embedding {
            model: "large".into(),
            values: target,
        };
        let exact = store
            .vector_search_with_mode(&query, 20, VectorSearchMode::Exact)
            .await
            .unwrap();
        let approximate = store
            .vector_search_with_mode(&query, 20, VectorSearchMode::Approximate)
            .await
            .unwrap();
        assert_eq!(exact[0].run_id, "v10049");
        assert_eq!(approximate[0].run_id, "v10049");
        assert!(
            approximate
                .iter()
                .filter(|a| exact.iter().any(|e| e.run_id == a.run_id))
                .count()
                >= 18
        );
        assert_eq!(store.vector_cache.entries.lock().await.len(), 1);
        let foreign = store.scoped(Scope {
            project: "foreign".into(),
            ..Scope::default()
        });
        assert!(foreign.vector_search(&query, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn replicas_replacements_restart_and_retention_invalidate_indexes() {
        let path = std::env::temp_dir().join(format!("{}.db", refract_core::id("ann-test")));
        let url = format!("sqlite://{}", path.display());
        let store = Store::open(&url).await.unwrap();
        let mut a = Run::new("first");
        a.id = "a".into();
        let mut b = Run::new("second");
        b.id = "b".into();
        store.insert_batch(&[a, b]).await.unwrap();
        let query = Embedding {
            model: "replica".into(),
            values: vec![1.0, 0.0],
        };
        store.put_embedding("a", &query).await.unwrap();
        store
            .put_embedding(
                "b",
                &Embedding {
                    model: "replica".into(),
                    values: vec![0.5, 0.5],
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .vector_search_with_mode(&query, 1, VectorSearchMode::Approximate)
                .await
                .unwrap()[0]
                .run_id,
            "a"
        );
        let replica = Store::open(&url).await.unwrap();
        replica
            .put_embedding(
                "a",
                &Embedding {
                    model: "replica".into(),
                    values: vec![-1.0, 0.0],
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .vector_search_with_mode(&query, 1, VectorSearchMode::Approximate)
                .await
                .unwrap()[0]
                .run_id,
            "b"
        );
        assert_eq!(
            replica
                .vector_search_with_mode(&query, 1, VectorSearchMode::Approximate)
                .await
                .unwrap()[0]
                .run_id,
            "b"
        );
        replica
            .retain_since(Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert!(
            store
                .vector_search_with_mode(&query, 1, VectorSearchMode::Approximate)
                .await
                .unwrap()
                .is_empty()
        );
        store.pool.close().await;
        replica.pool.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
