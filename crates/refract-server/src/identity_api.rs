use super::*;
use sha2::{Digest, Sha256};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateKey {
    role: Role,
    expires_in_days: u32,
}
pub(super) async fn keys(Extension(store): Extension<Store>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"keys":store.keys().await.map_err(internal)?})))
}
pub(super) async fn create_key(
    Extension(store): Extension<Store>,
    Json(req): Json<CreateKey>,
) -> ApiResult<Json<Value>> {
    if !(1..=365).contains(&req.expires_in_days) {
        return Err(invalid("key lifetime must be 1..365 days"));
    }
    let id = refract_core::id("key");
    let key = format!("rfr_{}{}", refract_core::id(""), refract_core::id(""));
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let expiry = chrono::Utc::now().timestamp() + i64::from(req.expires_in_days) * 86400;
    let role = match req.role {
        Role::Reader => "reader",
        Role::Writer => "writer",
        Role::Admin => "admin",
    };
    store
        .create_key(&id, &digest, role, expiry)
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({"id":id,"key":key,"role":role,"expires_at":expiry}),
    ))
}
pub(super) async fn revoke_key(
    Extension(store): Extension<Store>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if !store.revoke_key(&id).await.map_err(internal)? {
        return Err(ApiError(StatusCode::NOT_FOUND, "key not found".into()));
    }
    Ok(Json(json!({"revoked":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Provision {
    subject: String,
    role: Role,
    enabled: bool,
}
pub(super) async fn provision(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Json(req): Json<Provision>,
) -> ApiResult<Json<Value>> {
    let oidc = state
        .security
        .oidc
        .as_ref()
        .ok_or_else(|| invalid("OIDC is not configured"))?;
    let role = match req.role {
        Role::Reader => "reader",
        Role::Writer => "writer",
        Role::Admin => "admin",
    };
    if req.subject.is_empty() || req.subject.len() > 512 {
        return Err(invalid("invalid subject"));
    }
    if !store
        .provision(&oidc.issuer, &req.subject, role, req.enabled)
        .await
        .map_err(internal)?
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "subject is already provisioned".into(),
        ));
    }
    Ok(Json(
        json!({"subject":req.subject,"enabled":req.enabled,"role":role}),
    ))
}
