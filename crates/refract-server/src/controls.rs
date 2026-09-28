use super::*;
use refract_storage::{Embedding, VectorMatch};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VectorRequest {
    embedding: Embedding,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    mode: refract_storage::VectorSearchMode,
}
fn default_limit() -> usize {
    20
}
pub(super) async fn vector_search(
    Extension(store): Extension<Store>,
    Json(req): Json<VectorRequest>,
) -> ApiResult<Json<Vec<VectorMatch>>> {
    req.embedding.validate().map_err(invalid)?;
    if !(1..=100).contains(&req.limit) {
        return Err(invalid("limit must be 1..100"));
    }
    Ok(Json(
        store
            .vector_search_with_mode(&req.embedding, req.limit, req.mode)
            .await
            .map_err(internal)?,
    ))
}
pub(super) async fn embedding(
    Extension(store): Extension<Store>,
    Path(id): Path<String>,
    Json(req): Json<Embedding>,
) -> ApiResult<Json<Value>> {
    req.validate().map_err(invalid)?;
    if !store.put_embedding(&id, &req).await.map_err(internal)? {
        return Err(ApiError(StatusCode::NOT_FOUND, "run not found".into()));
    }
    Ok(Json(
        json!({"run_id":id,"model":req.model,"dimensions":req.values.len()}),
    ))
}
/// Bounded NDJSON page; same pagination and ordering as the JSON audit endpoint.
pub(super) async fn audit_export(
    Extension(store): Extension<Store>,
    Query(page): Query<Pagination>,
) -> ApiResult<Response> {
    let limit = page.limit.unwrap_or(1000);
    let offset = page.offset.unwrap_or(0);
    if !(1..=1000).contains(&limit) || offset < 0 {
        return Err(invalid("invalid pagination"));
    }
    let mut output = String::new();
    for entry in store.audit_log(limit, offset).await.map_err(internal)? {
        output.push_str(&serde_json::to_string(&entry).map_err(internal)?);
        output.push('\n');
    }
    Ok((
        [
            ("content-type", "application/x-ndjson"),
            (
                "content-disposition",
                "attachment; filename=\"audit.ndjson\"",
            ),
        ],
        output,
    )
        .into_response())
}
pub(super) async fn audit_expire(
    Extension(store): Extension<Store>,
    Json(req): Json<RetentionRequest>,
) -> ApiResult<Json<Value>> {
    if !(1..=36500).contains(&req.days) {
        return Err(invalid("audit retention days must be 1..36500"));
    }
    let before = chrono::Utc::now() - chrono::Duration::days(i64::from(req.days));
    Ok(Json(
        json!({"deleted":store.expire_audit(before).await.map_err(internal)?,"before":before}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RotationRequest {
    limit: i64,
}
pub(super) async fn rotate(
    Extension(store): Extension<Store>,
    Json(req): Json<RotationRequest>,
) -> ApiResult<Json<Value>> {
    if !(1..=1000).contains(&req.limit) {
        return Err(invalid("rotation limit must be 1..1000"));
    }
    Ok(Json(
        json!({"rotated":store.rotate_encryption(req.limit).await.map_err(internal)?}),
    ))
}
