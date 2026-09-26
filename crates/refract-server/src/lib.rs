mod delivery;
pub mod security;

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use refract_core::Run;
use refract_diff::{SemanticOptions, compare_semantic};
use refract_storage::{Encryption, RunFilter, SnapshotConflict, Store, StoreOptions};
use security::{Role, Security};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Instant,
};
use tower_http::services::ServeDir;

type ApiResult<T> = Result<T, ApiError>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn internal(_error: impl std::fmt::Display) -> ApiError {
    eprintln!("server operation failed; inspect database and service health");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".into(),
    )
}
fn invalid(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
async fn load(store: &Store, id: &str) -> ApiResult<Run> {
    store
        .get(id)
        .await
        .map_err(internal)?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "run not found".into()))
}
#[derive(Clone)]
struct AppState {
    store: Store,
    security: Security,
    redaction: refract_collector::RedactionPolicy,
    requests: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}
/// Development router with a local admin identity. Use router_with_security for shared services.
pub fn router(store: Store) -> Router {
    router_with_security(store, Security::default())
}
pub fn router_with_security(store: Store, security: Security) -> Router {
    router_with_policy(
        store,
        security,
        refract_collector::RedactionPolicy::default(),
    )
}
fn router_with_policy(
    store: Store,
    security: Security,
    redaction: refract_collector::RedactionPolicy,
) -> Router {
    let state = AppState {
        store,
        security,
        redaction,
        requests: Arc::default(),
    };
    Router::new()
        .route("/v1/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/v1/ready", get(ready))
        .route("/v1/runs", get(list).post(create))
        .route("/v1/runs/batch", post(batch))
        .route("/v1/search", get(search))
        .route("/v1/runs/{id}", get(inspect))
        .route("/v1/runs/{id}/events", get(events))
        .route("/v1/runs/{id}/metrics", get(metrics))
        .route("/v1/runs/{id}/similar", get(similar))
        .route("/v1/runs/{id}/replay", post(replay))
        .route("/v1/runs/{id}/fork", post(fork))
        .route("/v1/runs/{id}/artifact", get(export))
        .route("/v1/diff", post(diff))
        .route("/v1/eval", post(evaluate))
        .route("/v1/admin/audit", get(audit_log))
        .route("/v1/admin/retention", post(retention))
        .route("/v1/admin/outbox", get(outbox))
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .with_state(state)
}
async fn authorize(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if matches!(path.as_str(), "/v1/health" | "/v1/ready") {
        return next.run(request).await;
    }
    let bearer = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let Some(identity) = state.security.authenticate(bearer) else {
        return ApiError(
            StatusCode::UNAUTHORIZED,
            "valid Bearer API key required".into(),
        )
        .into_response();
    };
    let store = state.store.scoped(identity.scope);
    let method = request.method().clone();
    let readonly = method == Method::GET
        || method == Method::HEAD
        || path == "/v1/diff"
        || path == "/v1/eval"
        || path.ends_with("/replay");
    let forbidden = path.starts_with("/v1/admin/") && identity.role != Role::Admin
        || !readonly && identity.role == Role::Reader;
    let limited = {
        let mut requests = state.requests.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = requests
            .entry(identity.id.clone())
            .or_insert((Instant::now(), 0));
        if start.elapsed().as_secs() >= 60 {
            *start = Instant::now();
            *count = 0;
        }
        *count = count.saturating_add(1);
        *count > state.security.requests_per_minute
    };
    request.extensions_mut().insert(store.clone());
    request.extensions_mut().insert(state.redaction.clone());
    let response = if limited {
        (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "60")],
            Json(json!({"error":"API key rate limit exceeded"})),
        )
            .into_response()
    } else if forbidden {
        ApiError(
            StatusCode::FORBIDDEN,
            "API key role does not permit this operation".into(),
        )
        .into_response()
    } else {
        next.run(request).await
    };
    if let Err(error) = store
        .audit(
            &identity.id,
            method.as_str(),
            &path,
            response.status().as_u16(),
        )
        .await
    {
        return internal(error).into_response();
    }
    response
}
async fn ready(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    s.store.ready().await.map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "database unavailable".into(),
        )
    })?;
    Ok(Json(json!({"status":"ready"})))
}
async fn list(Extension(s): Extension<Store>) -> ApiResult<Json<Vec<Run>>> {
    Ok(Json(s.list().await.map_err(internal)?))
}
async fn search(
    Extension(s): Extension<Store>,
    Query(filter): Query<RunFilter>,
) -> ApiResult<Json<refract_storage::SearchPage>> {
    filter.validate().map_err(invalid)?;
    Ok(Json(s.search(&filter).await.map_err(internal)?))
}
async fn create(
    Extension(s): Extension<Store>,
    Extension(policy): Extension<refract_collector::RedactionPolicy>,
    Json(run): Json<Run>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let run = policy.normalize(run).map_err(invalid)?;
    if !s.insert(&run).await.map_err(internal)? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "run id already exists".into(),
        ));
    }
    Ok((StatusCode::CREATED, Json(run)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchRequest {
    runs: Vec<Run>,
}
async fn batch(
    Extension(s): Extension<Store>,
    Extension(policy): Extension<refract_collector::RedactionPolicy>,
    Json(request): Json<BatchRequest>,
) -> ApiResult<Json<refract_storage::BatchReceipt>> {
    if request.runs.is_empty() || request.runs.len() > 1000 {
        return Err(invalid("batch must contain 1..1000 runs"));
    }
    let runs = request
        .runs
        .into_iter()
        .map(|run| policy.normalize(run))
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(invalid)?;
    Ok(Json(s.insert_batch(&runs).await.map_err(|error| {
        if error.downcast_ref::<SnapshotConflict>().is_some() {
            ApiError(StatusCode::CONFLICT, error.to_string())
        } else {
            internal(error)
        }
    })?))
}
async fn inspect(Extension(s): Extension<Store>, Path(id): Path<String>) -> ApiResult<Json<Run>> {
    Ok(Json(load(&s, &id).await?))
}
async fn events(Extension(s): Extension<Store>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(load(&s, &id).await?.events)))
}
async fn metrics(
    Extension(s): Extension<Store>,
    Path(id): Path<String>,
) -> ApiResult<Json<refract_core::metrics::Metrics>> {
    Ok(Json(refract_core::metrics(&load(&s, &id).await?)))
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Pagination {
    limit: Option<i64>,
    offset: Option<i64>,
}
fn tokens(run: &Run) -> BTreeSet<String> {
    let text = format!(
        "{} {}",
        run.name,
        run.events
            .iter()
            .map(|e| format!("{} {} {}", e.name, e.input, e.output))
            .collect::<Vec<_>>()
            .join(" ")
    );
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_owned)
        .collect()
}
async fn similar(
    Extension(s): Extension<Store>,
    Path(id): Path<String>,
    Query(page): Query<Pagination>,
) -> ApiResult<Json<Value>> {
    let limit = page.limit.unwrap_or(10);
    if !(1..=100).contains(&limit) || page.offset.unwrap_or(0) != 0 {
        return Err(invalid(
            "similar limit must be 1..100; offset is unsupported",
        ));
    }
    let original = tokens(&load(&s, &id).await?);
    // Bounded lexical search is explicit: no hidden embedding service or unbounded payload scan.
    let candidates = s
        .search(&RunFilter {
            limit: Some(1000),
            ..Default::default()
        })
        .await
        .map_err(internal)?;
    let mut results = candidates
        .runs
        .into_iter()
        .filter(|run| run.id != id)
        .map(|run| {
            let words = tokens(&run);
            let union = original.union(&words).count();
            let score = if union == 0 {
                0.0
            } else {
                original.intersection(&words).count() as f64 / union as f64
            };
            (score, run)
        })
        .collect::<Vec<_>>();
    results.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    Ok(Json(
        json!({"method":"lexical-jaccard-v1", "candidate_limit":1000,"total_candidates":candidates.total,"runs":results.into_iter().take(limit as usize).map(|(score,run)| json!({"score":score,"run":run})).collect::<Vec<_>>() }),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayRequest {
    mode: String,
}
async fn replay(
    Extension(s): Extension<Store>,
    Path(id): Path<String>,
    Json(req): Json<ReplayRequest>,
) -> ApiResult<Json<Value>> {
    if req.mode != "exact" {
        return Err(invalid(
            "only exact recorded playback is supported by this endpoint; use local rerun handlers for executable replay",
        ));
    }
    Ok(Json(
        json!({"mode":"exact","steps":refract_replay::exact(&load(&s, &id).await?).map_err(invalid)?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForkRequest {
    from_event: String,
}
async fn fork(
    Extension(s): Extension<Store>,
    Path(id): Path<String>,
    Json(req): Json<ForkRequest>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let branch = refract_replay::fork(&load(&s, &id).await?, &req.from_event).map_err(invalid)?;
    s.insert(&branch).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(branch)))
}
async fn export(Extension(s): Extension<Store>, Path(id): Path<String>) -> ApiResult<Response> {
    let bytes = refract_artifact::pack(&load(&s, &id).await?).map_err(internal)?;
    Ok((
        [
            ("content-type", "application/vnd.refract.rfr"),
            ("content-disposition", "attachment; filename=execution.rfr"),
        ],
        bytes,
    )
        .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffRequest {
    left: String,
    right: String,
    #[serde(default)]
    semantic: bool,
    #[serde(default)]
    options: SemanticOptions,
}
async fn diff(
    Extension(s): Extension<Store>,
    Json(req): Json<DiffRequest>,
) -> ApiResult<Json<Value>> {
    req.options.validate().map_err(invalid)?;
    let left = load(&s, &req.left).await?;
    let right = load(&s, &req.right).await?;
    let differences = refract_diff::compare(&left, &right);
    Ok(Json(
        json!({"first_divergence":differences.first().map(|d|d.index),"differences":differences,"metric_changes":refract_core::compare_metrics(&left,&right),"semantic_report":req.semantic.then(||compare_semantic(&left,&right,&req.options))}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalPair {
    name: String,
    left: String,
    right: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalRequest {
    pairs: Vec<EvalPair>,
    #[serde(default)]
    options: SemanticOptions,
}
async fn evaluate(
    Extension(s): Extension<Store>,
    Json(req): Json<EvalRequest>,
) -> ApiResult<Json<Value>> {
    req.options.validate().map_err(invalid)?;
    if req.pairs.is_empty() || req.pairs.len() > 100 {
        return Err(invalid("evaluation requires 1..100 pairs"));
    }
    let mut results = vec![];
    let mut passed = 0;
    for pair in req.pairs {
        let report = compare_semantic(
            &load(&s, &pair.left).await?,
            &load(&s, &pair.right).await?,
            &req.options,
        );
        passed += usize::from(report.passed);
        results.push(json!({"name":pair.name,"left":pair.left,"right":pair.right,"report":report}));
    }
    Ok(Json(
        json!({"passed":passed==results.len(),"total":results.len(),"regressions":results.len()-passed,"equivalent":passed,"results":results}),
    ))
}
async fn audit_log(
    Extension(s): Extension<Store>,
    Query(page): Query<Pagination>,
) -> ApiResult<Json<Value>> {
    let limit = page.limit.unwrap_or(100);
    let offset = page.offset.unwrap_or(0);
    if !(1..=1000).contains(&limit) || offset < 0 {
        return Err(invalid("invalid pagination"));
    }
    Ok(Json(
        json!({"entries":s.audit_log(limit,offset).await.map_err(internal)?,"limit":limit,"offset":offset}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionRequest {
    days: u32,
}
async fn retention(
    Extension(s): Extension<Store>,
    Json(req): Json<RetentionRequest>,
) -> ApiResult<Json<Value>> {
    if !(1..=36500).contains(&req.days) {
        return Err(invalid("retention days must be 1..36500"));
    }
    let before = chrono::Utc::now() - chrono::Duration::days(i64::from(req.days));
    Ok(Json(
        json!({"deleted":s.retain_since(before).await.map_err(internal)?,"before":before}),
    ))
}
async fn outbox(Extension(s): Extension<Store>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"pending":s.outbox_pending().await.map_err(internal)?}),
    ))
}
pub async fn serve() -> anyhow::Result<()> {
    let url = security::secret("REFRACT_DATABASE_URL")?.unwrap_or("sqlite://refract.db".into());
    let address = std::env::var("REFRACT_BIND").unwrap_or("127.0.0.1:8000".into());
    let security = Security::from_env()?;
    let delivery = delivery::Delivery::from_env()?;
    let encryption = security::secret("REFRACT_ENCRYPTION_KEY")?
        .map(|key| Encryption::from_base64(&key))
        .transpose()?;
    let mode = std::env::var("REFRACT_MODE").unwrap_or("local".into());
    security::validate_mode(
        &mode,
        &security,
        encryption.is_some(),
        std::env::var("REFRACT_TLS_TERMINATED").as_deref() == Ok("1"),
    )?;
    delivery.validate_production(&mode)?;
    let store = Store::open_with_options(
        &url,
        StoreOptions {
            encryption,
            outbox_targets: delivery.targets(),
        },
    )
    .await?;
    let worker_store = store.clone();
    let retention = security.retention;
    let worker = tokio::spawn(async move {
        let mut last_retention = Instant::now() - std::time::Duration::from_secs(3600);
        loop {
            if last_retention.elapsed().as_secs() >= 3600 {
                if let Some(age) = retention {
                    let before = chrono::Utc::now()
                        - chrono::Duration::from_std(age).expect("validated retention");
                    match worker_store.scopes().await {
                        Ok(scopes) => {
                            for scope in scopes {
                                let scoped = worker_store.scoped(scope);
                                match scoped.retain_since(before).await {
                                    Ok(deleted) if deleted > 0 => {
                                        let _ = scoped
                                            .audit("retention-worker", "DELETE", "runs", 200)
                                            .await;
                                    }
                                    Ok(_) => (),
                                    Err(_) => {
                                        eprintln!("retention failed; inspect database health")
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            eprintln!("retention enumeration failed; inspect database health")
                        }
                    }
                }
                last_retention = Instant::now();
            }
            match delivery.process_one(&worker_store).await {
                Ok(true) => continue,
                Ok(false) => (),
                Err(e) => eprintln!("outbox delivery failed: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    if security.identities.is_empty() {
        eprintln!("Local unauthenticated mode; configure REFRACT_API_KEYS for shared deployments");
    }
    let ui = std::env::var("REFRACT_UI_DIR").unwrap_or("apps/viewer/dist".into());
    let app = router_with_policy(
        store,
        security,
        refract_collector::RedactionPolicy::from_env()?,
    )
    .fallback_service(ServeDir::new(ui));
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Refract listening on {address}");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    worker.abort();
    result?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => (),
            _ = terminate.recv() => (),
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
#[cfg(test)]
mod tests;
