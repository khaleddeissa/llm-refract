use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use refract_core::Run;
use refract_storage::Store;
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::ServeDir;
type ApiResult<T> = Result<T, ApiError>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn internal(e: impl std::fmt::Display) -> ApiError {
    eprintln!("storage error: {e}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal storage error".into(),
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
pub fn router(store: Store) -> Router {
    Router::new()
        .route("/v1/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/v1/ready", get(ready))
        .route("/v1/runs", get(list).post(create))
        .route("/v1/runs/{id}", get(inspect))
        .route("/v1/runs/{id}/events", get(events))
        .route("/v1/runs/{id}/replay", post(replay))
        .route("/v1/runs/{id}/fork", post(fork))
        .route("/v1/runs/{id}/artifact", get(export))
        .route("/v1/diff", post(diff))
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(store)
}
async fn ready(State(s): State<Store>) -> ApiResult<Json<Value>> {
    s.ready().await.map_err(internal)?;
    Ok(Json(json!({"status":"ready"})))
}
async fn list(State(s): State<Store>) -> ApiResult<Json<Vec<Run>>> {
    Ok(Json(s.list().await.map_err(internal)?))
}
async fn create(
    State(s): State<Store>,
    Json(run): Json<Run>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let run = refract_collector::normalize(run).map_err(invalid)?;
    if !s.insert(&run).await.map_err(internal)? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "run id already exists".into(),
        ));
    }
    Ok((StatusCode::CREATED, Json(run)))
}
async fn inspect(State(s): State<Store>, Path(id): Path<String>) -> ApiResult<Json<Run>> {
    Ok(Json(load(&s, &id).await?))
}
async fn events(State(s): State<Store>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(load(&s, &id).await?.events)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayRequest {
    mode: String,
}
async fn replay(
    State(s): State<Store>,
    Path(id): Path<String>,
    Json(req): Json<ReplayRequest>,
) -> ApiResult<Json<Value>> {
    if req.mode != "exact" {
        return Err(invalid("only exact recorded playback is supported"));
    }
    let run = load(&s, &id).await?;
    Ok(Json(
        json!({"mode":"exact","steps":refract_replay::exact(&run).map_err(invalid)?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForkRequest {
    from_event: String,
}
async fn fork(
    State(s): State<Store>,
    Path(id): Path<String>,
    Json(req): Json<ForkRequest>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let branch = refract_replay::fork(&load(&s, &id).await?, &req.from_event).map_err(invalid)?;
    s.insert(&branch).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(branch)))
}
async fn export(State(s): State<Store>, Path(id): Path<String>) -> ApiResult<Response> {
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
struct DiffRequest {
    left: String,
    right: String,
}
async fn diff(State(s): State<Store>, Json(req): Json<DiffRequest>) -> ApiResult<Json<Value>> {
    let differences =
        refract_diff::compare(&load(&s, &req.left).await?, &load(&s, &req.right).await?);
    Ok(Json(
        json!({"first_divergence":differences.first().map(|d|d.index),"differences":differences}),
    ))
}
pub async fn serve() -> anyhow::Result<()> {
    let url = std::env::var("REFRACT_DATABASE_URL").unwrap_or("sqlite://refract.db".into());
    let address = std::env::var("REFRACT_BIND").unwrap_or("127.0.0.1:8000".into());
    let store = Store::open(&url).await?;
    let ui = std::env::var("REFRACT_UI_DIR").unwrap_or("ui/dist".into());
    let app = router(store).fallback_service(ServeDir::new(ui));
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Refract listening on {address}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn ingest_replay_export_and_conflict() {
        let app = router(Store::open("sqlite::memory:").await.unwrap());
        let fixture = include_str!("../../../tests/fixtures/simple-run/execution.json");
        for expected in [StatusCode::CREATED, StatusCode::CONFLICT] {
            let req = Request::post("/v1/runs")
                .header("content-type", "application/json")
                .body(Body::from(fixture))
                .unwrap();
            assert_eq!(app.clone().oneshot(req).await.unwrap().status(), expected);
        }
        let req = Request::post("/v1/runs/demo-1/replay")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"mode":"live"}"#))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        let req = Request::get("/v1/runs/demo-1/artifact")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = to_bytes(res.into_body(), 1_000_000).await.unwrap();
        assert_eq!(refract_artifact::unpack(&bytes).unwrap().events.len(), 2);
    }
}
