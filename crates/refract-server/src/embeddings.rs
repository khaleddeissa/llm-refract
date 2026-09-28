//! Operator-owned provider profiles, tenant-owned selections, and durable embedding execution.
use super::*;
use anyhow::{Result, ensure};
use refract_storage::{Embedding, EmbeddingSetting, Scope};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Protocol {
    Openai,
    Voyage,
    Ollama,
    Cohere,
    Gemini,
    Vertex,
    Custom,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    id: String,
    label: String,
    protocol: Protocol,
    endpoint: String,
    model: String,
    dimensions: usize,
    #[serde(default)]
    request_dimensions: bool,
    #[serde(default)]
    credential_env: Option<String>,
    #[serde(default = "authorization_header")]
    auth_header: String,
    #[serde(default)]
    scopes: Vec<Scope>,
    #[serde(default)]
    request_template: Option<Value>,
    #[serde(default)]
    response_pointer: Option<String>,
}
fn authorization_header() -> String {
    "authorization".into()
}
impl Profile {
    fn namespace(&self) -> String {
        // Credential rotation and labels do not change the vector space; preprocessing does.
        let hash = Sha256::digest(
            json!([
                self.protocol,
                self.endpoint,
                self.model,
                self.dimensions,
                self.request_dimensions,
                self.request_template,
                self.response_pointer,
                "run-text-v1"
            ])
            .to_string()
            .as_bytes(),
        );
        format!("{}:{hash:x}", self.id)
    }
    fn allowed(&self, scope: &Scope) -> bool {
        self.scopes.is_empty() || self.scopes.contains(scope)
    }
    fn public(&self) -> Value {
        json!({"id":self.id,"label":self.label,"protocol":self.protocol,"model":self.model,"dimensions":self.dimensions,"namespace":self.namespace()})
    }
    fn body(&self, text: &str, query: bool) -> Value {
        let task = if query { "query" } else { "document" };
        match self.protocol {
            Protocol::Openai => {
                let mut body = json!({"model":self.model,"input":[text],"encoding_format":"float"});
                if self.request_dimensions {
                    body["dimensions"] = self.dimensions.into();
                }
                body
            }
            Protocol::Voyage => {
                let mut body =
                    json!({"model":self.model,"input":[text],"input_type":task,"truncation":false});
                if self.request_dimensions {
                    body["output_dimension"] = self.dimensions.into();
                }
                body
            }
            Protocol::Ollama => {
                let mut body = json!({"model":self.model,"input":[text],"truncate":false});
                if self.request_dimensions {
                    body["dimensions"] = self.dimensions.into();
                }
                body
            }
            Protocol::Cohere => {
                let mut body = json!({"model":self.model,"texts":[text],"input_type":format!("search_{task}"),"embedding_types":["float"],"truncate":"NONE"});
                if self.request_dimensions {
                    body["output_dimension"] = self.dimensions.into();
                }
                body
            }
            Protocol::Gemini => {
                let mut body = json!({"model":self.model,"content":{"parts":[{"text":text}]},"taskType":if query {"RETRIEVAL_QUERY"} else {"RETRIEVAL_DOCUMENT"}});
                if self.request_dimensions {
                    body["outputDimensionality"] = self.dimensions.into();
                }
                body
            }
            Protocol::Vertex => {
                let mut body = json!({"instances":[{"content":text,"task_type":if query {"RETRIEVAL_QUERY"} else {"RETRIEVAL_DOCUMENT"}}],"parameters":{"autoTruncate":false}});
                if self.request_dimensions {
                    body["parameters"]["outputDimensionality"] = self.dimensions.into();
                }
                body
            }
            Protocol::Custom => substitute(
                self.request_template
                    .as_ref()
                    .expect("validated custom template"),
                text,
                &self.model,
                task,
            ),
        }
    }
    fn pointer(&self) -> &str {
        match self.protocol {
            Protocol::Openai | Protocol::Voyage => "/data/0/embedding",
            Protocol::Ollama => "/embeddings/0",
            Protocol::Cohere => "/embeddings/float/0",
            Protocol::Gemini => "/embedding/values",
            Protocol::Vertex => "/predictions/0/embeddings/values",
            Protocol::Custom => self
                .response_pointer
                .as_deref()
                .expect("validated custom pointer"),
        }
    }
}
fn substitute(value: &Value, text: &str, model: &str, task: &str) -> Value {
    match value {
        Value::String(s) if s == "$text" => text.into(),
        Value::String(s) if s == "$model" => model.into(),
        Value::String(s) if s == "$task" => task.into(),
        Value::Array(values) => values
            .iter()
            .map(|v| substitute(v, text, model, task))
            .collect(),
        Value::Object(values) => values
            .iter()
            .map(|(k, v)| (k.clone(), substitute(v, text, model, task)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
        _ => value.clone(),
    }
}

#[derive(Clone)]
pub(super) struct Registry {
    profiles: Arc<BTreeMap<String, Profile>>,
    client: reqwest::Client,
}
impl Default for Registry {
    fn default() -> Self {
        Self::parse("[]", false).expect("empty embedding registry")
    }
}
impl Registry {
    pub(super) fn from_env(production: bool) -> Result<Self> {
        Self::parse(
            &security::secret("REFRACT_EMBEDDING_PROFILES")?.unwrap_or_else(|| "[]".into()),
            production,
        )
    }
    fn parse(config: &str, production: bool) -> Result<Self> {
        let profiles: Vec<Profile> = serde_json::from_str(config)?;
        ensure!(
            profiles.len() <= 100,
            "at most 100 operator embedding profiles"
        );
        let mut registry = BTreeMap::new();
        for profile in profiles {
            ensure!(
                !profile.id.is_empty()
                    && profile.id.len() <= 100
                    && profile
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                "invalid embedding profile id"
            );
            ensure!(
                !profile.label.is_empty()
                    && profile.label.len() <= 200
                    && !profile.model.is_empty()
                    && profile.model.len() <= 256,
                "invalid embedding label or model"
            );
            ensure!(
                (1..=4096).contains(&profile.dimensions),
                "embedding dimensions must be 1..4096"
            );
            let url = reqwest::Url::parse(&profile.endpoint)?;
            ensure!(
                url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none(),
                "embedding endpoint must not contain credentials or fragment"
            );
            ensure!(
                url.scheme() == "https" || (!production && url.scheme() == "http"),
                "embedding endpoints require HTTPS in production"
            );
            ensure!(
                matches!(
                    profile.auth_header.as_str(),
                    "authorization" | "api-key" | "x-goog-api-key" | "x-api-key"
                ),
                "unsupported embedding credential header"
            );
            if let Some(name) = &profile.credential_env {
                ensure!(
                    !name.is_empty()
                        && name.len() <= 200
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                    "invalid credential environment variable name"
                );
            }
            for scope in &profile.scopes {
                scope.validate()?;
            }
            if matches!(profile.protocol, Protocol::Custom) {
                ensure!(
                    profile
                        .request_template
                        .as_ref()
                        .is_some_and(Value::is_object)
                        && profile
                            .response_pointer
                            .as_ref()
                            .is_some_and(|p| p.starts_with('/')),
                    "custom protocol needs request_template and response_pointer"
                );
            }
            ensure!(
                registry.insert(profile.id.clone(), profile).is_none(),
                "duplicate embedding profile"
            );
        }
        Ok(Self {
            profiles: Arc::new(registry),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(25))
                .build()?,
        })
    }
    fn profile(&self, id: &str, scope: &Scope) -> Result<&Profile> {
        self.profiles
            .get(id)
            .filter(|p| p.allowed(scope))
            .ok_or_else(|| anyhow::anyhow!("embedding profile is unavailable for this project"))
    }
    async fn embed(&self, profile: &Profile, text: &str, query: bool) -> Result<Vec<f64>> {
        let mut request = self
            .client
            .post(&profile.endpoint)
            .json(&profile.body(text, query));
        if let Some(name) = &profile.credential_env {
            let key = security::secret(name)?
                .ok_or_else(|| anyhow::anyhow!("embedding credential is unavailable"))?;
            let value = if profile.auth_header == "authorization" {
                format!("Bearer {key}")
            } else {
                key
            };
            let mut header = reqwest::header::HeaderValue::from_str(&value)?;
            header.set_sensitive(true);
            request = request.header(&profile.auth_header, header);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("embedding provider request failed"))?;
        ensure!(
            response.status().is_success(),
            "embedding provider returned HTTP {}",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 1024 * 1024,
                "embedding response exceeds 1 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let payload: Value = serde_json::from_slice(&bytes)?;
        let values: Vec<f64> = serde_json::from_value(
            payload
                .pointer(profile.pointer())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("embedding response has no vector"))?,
        )?;
        ensure!(
            values.len() == profile.dimensions,
            "embedding dimension mismatch"
        );
        Embedding {
            model: profile.namespace(),
            values: values.clone(),
        }
        .validate()?;
        Ok(values)
    }
    pub(super) async fn process_one(&self, store: &Store) -> Result<bool> {
        let Some(job) = store.claim_embedding_job().await? else {
            return Ok(false);
        };
        let scoped = store.scoped(job.scope.clone());
        let work = async {
            let profile = self.profile(&job.profile, &job.scope)?;
            ensure!(
                profile.namespace() == job.model,
                "embedding profile changed; re-save project settings"
            );
            let run = scoped
                .get(&job.run_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("run removed"))?;
            let text = run_text(&run);
            // Bound cost and lease time; a document is four chunks, averaged after normalization.
            let chars: Vec<char> = text.chars().collect();
            let mut mean = vec![0.0; profile.dimensions];
            for chunk in chars.chunks(2000) {
                let vector = self
                    .embed(profile, &chunk.iter().collect::<String>(), false)
                    .await?;
                let norm = vector.iter().map(|v| v * v).sum::<f64>().sqrt();
                for (a, b) in mean.iter_mut().zip(vector) {
                    *a += b / norm;
                }
            }
            Embedding {
                model: profile.namespace(),
                values: mean.clone(),
            }
            .validate()?;
            Ok::<_, anyhow::Error>(mean)
        };
        let values = match tokio::time::timeout(Duration::from_secs(100), work).await {
            Ok(Ok(values)) => Some(values),
            _ => {
                eprintln!(
                    "embedding job failed; inspect profile configuration and provider health"
                );
                None
            }
        };
        scoped.finish_embedding_job(&job, values).await?;
        Ok(true)
    }
}
fn run_text(run: &Run) -> String {
    // Explicit, versioned representation: no arbitrary attributes/headers or unredacted source.
    let mut text = run.name.clone();
    for event in &run.events {
        text.push_str(&format!(
            "\n{}\n{}\n{}",
            event.name, event.input, event.output
        ));
        if text.chars().count() >= 8000 {
            break;
        }
    }
    text.chars().take(8000).collect()
}

pub(super) async fn models(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
) -> Json<Value> {
    Json(
        json!({"models":state.embeddings.profiles.values().filter(|p| p.allowed(store.scope())).map(Profile::public).collect::<Vec<_>>()}),
    )
}
pub(super) async fn settings(Extension(store): Extension<Store>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"profiles":store.embedding_settings().await.map_err(internal)?,"jobs":store.embedding_job_counts().await.map_err(internal)?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    profile: String,
    is_default: bool,
    auto_index: bool,
}
pub(super) async fn configure(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Json(selections): Json<Vec<Selection>>,
) -> ApiResult<Json<Value>> {
    let settings: Vec<_> = selections
        .into_iter()
        .map(|s| {
            let profile = state
                .embeddings
                .profile(&s.profile, store.scope())
                .map_err(invalid)?;
            Ok(EmbeddingSetting {
                profile: s.profile,
                model: profile.namespace(),
                is_default: s.is_default,
                auto_index: s.auto_index,
            })
        })
        .collect::<ApiResult<_>>()?;
    store
        .set_embedding_settings(&settings)
        .await
        .map_err(invalid)?;
    Ok(Json(json!({"profiles":settings})))
}
pub(super) async fn reindex(Extension(store): Extension<Store>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"queued":store.reindex_embeddings(true).await.map_err(internal)?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TextSearch {
    query: String,
    profile: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    mode: refract_storage::VectorSearchMode,
}
fn default_limit() -> usize {
    20
}
pub(super) async fn search(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Json(req): Json<TextSearch>,
) -> ApiResult<Json<Value>> {
    if req.query.trim().is_empty() || req.query.len() > 8000 || !(1..=100).contains(&req.limit) {
        return Err(invalid("text query must be 1..8000 bytes and limit 1..100"));
    }
    let settings = store.embedding_settings().await.map_err(internal)?;
    let selection = settings
        .iter()
        .find(|s| {
            req.profile
                .as_ref()
                .map_or(s.is_default, |id| &s.profile == id)
        })
        .ok_or_else(|| invalid("select an enabled project embedding profile"))?;
    let profile = state
        .embeddings
        .profile(&selection.profile, store.scope())
        .map_err(invalid)?;
    if profile.namespace() != selection.model {
        return Err(invalid(
            "embedding profile changed; re-save project settings and reindex",
        ));
    }
    let values = state
        .embeddings
        .embed(profile, &req.query, true)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                "embedding provider failed; inspect provider configuration".into(),
            )
        })?;
    let matches = store
        .vector_search_with_mode(
            &Embedding {
                model: selection.model.clone(),
                values,
            },
            req.limit,
            req.mode,
        )
        .await
        .map_err(internal)?;
    let mut runs = vec![];
    for found in &matches {
        if let Some(run) = store.get(&found.run_id).await.map_err(internal)? {
            runs.push(run);
        }
    }
    Ok(Json(
        json!({"runs":runs,"total":runs.len(),"matches":matches,"profile":selection.profile,"namespace":selection.model}),
    ))
}

#[cfg(test)]
mod tests;
