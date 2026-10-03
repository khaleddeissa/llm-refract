//! SCIM 2.0 Users and Groups, authorized by the existing scoped admin identity.
use super::*;
use anyhow::{bail, ensure};
use refract_storage::Directory;
use std::collections::{BTreeMap, BTreeSet};

const USER: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const GROUP: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
#[derive(Debug)]
pub(super) struct Error(StatusCode, String);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, [("content-type", "application/scim+json")], Json(json!({
            "schemas":["urn:ietf:params:scim:api:messages:2.0:Error"], "status":self.0.as_u16().to_string(), "detail":self.1
        }))).into_response()
    }
}
type Result<T> = std::result::Result<T, Error>;
fn bad(error: impl std::fmt::Display) -> Error {
    Error(StatusCode::BAD_REQUEST, error.to_string())
}
fn missing() -> Error {
    Error(StatusCode::NOT_FOUND, "SCIM resource not found".into())
}
fn response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [("content-type", "application/scim+json")],
        Json(value),
    )
        .into_response()
}
fn users(kind: &str) -> Result<bool> {
    match kind {
        "Users" => Ok(true),
        "Groups" => Ok(false),
        _ => Err(missing()),
    }
}
fn resources(directory: &Directory, user: bool) -> &BTreeMap<String, Value> {
    if user {
        &directory.users
    } else {
        &directory.groups
    }
}
fn resources_mut(directory: &mut Directory, user: bool) -> &mut BTreeMap<String, Value> {
    if user {
        &mut directory.users
    } else {
        &mut directory.groups
    }
}
fn issuer(state: &AppState) -> Result<&str> {
    state
        .security
        .oidc
        .as_ref()
        .map(|o| o.issuer.as_str())
        .ok_or_else(|| bad("configure OIDC before SCIM provisioning"))
}
fn validate(value: &mut Value, id: &str, user: bool, directory: &Directory) -> anyhow::Result<()> {
    ensure!(value.is_object(), "resource must be an object");
    let key = if user { "userName" } else { "displayName" };
    let name = value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 512)
        .ok_or_else(|| anyhow::anyhow!("{key} is required and must be at most 512 bytes"))?;
    for (other_id, other) in resources(directory, user) {
        ensure!(
            other_id == id
                || !other[key]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(name)),
            "{key} must be unique"
        );
    }
    if user {
        let subject = value["externalId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .ok_or_else(|| anyhow::anyhow!("externalId must be the immutable OIDC subject"))?;
        ensure!(
            directory
                .users
                .iter()
                .all(|(i, u)| i == id || u["externalId"] != subject),
            "externalId must be unique"
        );
        if let Some(old) = directory.users.get(id) {
            ensure!(old["externalId"] == subject, "externalId is immutable");
        }
        if value.get("active").is_none() {
            value["active"] = json!(true);
        }
        ensure!(value["active"].is_boolean(), "active must be boolean");
        // Authorization is derived from configured group mappings, never client-provided roles.
        value.as_object_mut().unwrap().remove("roles");
        value.as_object_mut().unwrap().remove("password");
        value.as_object_mut().unwrap().remove("groups");
    } else {
        if value.get("members").is_none() {
            value["members"] = json!([]);
        }
        let members = value["members"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("members must be an array"))?;
        ensure!(members.len() <= 10000, "too many group members");
        let mut seen = BTreeSet::new();
        for member in members.iter_mut() {
            let id = member["value"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("member value required"))?;
            ensure!(
                directory.users.contains_key(id),
                "member must identify a provisioned User"
            );
            ensure!(seen.insert(id.to_owned()), "duplicate member");
            *member = json!({"value":id});
        }
    }
    let created = directory
        .users
        .get(id)
        .or_else(|| directory.groups.get(id))
        .and_then(|v| v["meta"]["created"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    value["id"] = json!(id);
    value["schemas"] = json!([if user { USER } else { GROUP }]);
    value["meta"] = json!({"resourceType":if user {"User"} else {"Group"},"created":created,"lastModified":chrono::Utc::now().to_rfc3339()});
    ensure!(
        serde_json::to_vec(value)?.len() <= 1024 * 1024,
        "resource exceeds 1 MiB"
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Page {
    start_index: usize,
    count: usize,
    filter: Option<String>,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            start_index: 1,
            count: 100,
            filter: None,
        }
    }
}
pub(super) async fn list(
    Extension(store): Extension<Store>,
    Path(kind): Path<String>,
    Query(page): Query<Page>,
) -> Result<Response> {
    let user = users(&kind)?;
    if page.start_index == 0 || page.count > 1000 {
        return Err(bad("startIndex must be positive and count at most 1000"));
    }
    let filter = page
        .filter
        .as_deref()
        .map(parse_filter)
        .transpose()
        .map_err(bad)?;
    let directory = store.directory().await.map_err(|_| {
        Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "directory unavailable".into(),
        )
    })?;
    let matches: Vec<_> = resources(&directory, user)
        .values()
        .filter(|v| {
            filter.as_ref().is_none_or(|(k, e)| {
                if let (Some(a), Some(b)) = (v[k].as_str(), e.as_str()) {
                    if k == "userName" || k == "displayName" {
                        a.eq_ignore_ascii_case(b)
                    } else {
                        a == b
                    }
                } else {
                    v[k] == *e
                }
            })
        })
        .collect();
    let total = matches.len();
    Ok(response(
        StatusCode::OK,
        json!({"schemas":["urn:ietf:params:scim:api:messages:2.0:ListResponse"],"totalResults":total,"startIndex":page.start_index,"itemsPerPage":matches.iter().skip(page.start_index-1).take(page.count).count(),"Resources":matches.iter().skip(page.start_index-1).take(page.count).collect::<Vec<_>>()}),
    ))
}
fn parse_filter(filter: &str) -> anyhow::Result<(String, Value)> {
    let (key, value) = filter
        .split_once(" eq ")
        .ok_or_else(|| anyhow::anyhow!("supported filter: attribute eq value"))?;
    ensure!(
        ["id", "userName", "externalId", "displayName", "active"].contains(&key),
        "unsupported filter attribute"
    );
    let value: Value = serde_json::from_str(value)?;
    ensure!(
        if key == "active" {
            value.is_boolean()
        } else {
            value.is_string()
        },
        "invalid filter value"
    );
    Ok((key.into(), value))
}
pub(super) async fn get(
    Extension(store): Extension<Store>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response> {
    let directory = store.directory().await.map_err(bad)?;
    Ok(response(
        StatusCode::OK,
        resources(&directory, users(&kind)?)
            .get(&id)
            .cloned()
            .ok_or_else(missing)?,
    ))
}
pub(super) async fn create(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Path(kind): Path<String>,
    Json(mut value): Json<Value>,
) -> Result<Response> {
    let user = users(&kind)?;
    let id = refract_core::id("scim");
    let value = store
        .update_directory(issuer(&state)?, &state.security.scim_group_roles, |d| {
            validate(&mut value, &id, user, d)?;
            resources_mut(d, user).insert(id.clone(), value.clone());
            Ok(value)
        })
        .await
        .map_err(bad)?;
    let mut result = response(StatusCode::CREATED, value);
    result
        .headers_mut()
        .insert("location", format!("/scim/v2/{kind}/{id}").parse().unwrap());
    Ok(result)
}
pub(super) async fn replace(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Path((kind, id)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<Response> {
    modify(state, store, kind, id, value, false).await
}
pub(super) async fn patch(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Path((kind, id)): Path<(String, String)>,
    Json(value): Json<Value>,
) -> Result<Response> {
    modify(state, store, kind, id, value, true).await
}
async fn modify(
    state: AppState,
    store: Store,
    kind: String,
    id: String,
    mut value: Value,
    patch: bool,
) -> Result<Response> {
    let user = users(&kind)?;
    let value = store
        .update_directory(issuer(&state)?, &state.security.scim_group_roles, |d| {
            let old = resources(d, user)
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("resource not found"))?;
            if patch {
                value = patched(old.clone(), &value, user)?;
            }
            validate(&mut value, &id, user, d)?;
            resources_mut(d, user).insert(id, value.clone());
            Ok(value)
        })
        .await
        .map_err(|e| {
            if e.to_string() == "resource not found" {
                missing()
            } else {
                bad(e)
            }
        })?;
    Ok(response(StatusCode::OK, value))
}
fn patched(mut resource: Value, patch: &Value, user: bool) -> anyhow::Result<Value> {
    ensure!(
        patch["schemas"].as_array().is_some_and(|s| s
            .iter()
            .any(|s| s == "urn:ietf:params:scim:api:messages:2.0:PatchOp")),
        "PatchOp schema required"
    );
    let operations = patch["Operations"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Operations required"))?;
    ensure!(
        !operations.is_empty() && operations.len() <= 100,
        "require 1..100 patch operations"
    );
    for operation in operations {
        let op = operation["op"].as_str().unwrap_or("").to_ascii_lowercase();
        ensure!(
            ["add", "replace", "remove"].contains(&op.as_str()),
            "invalid patch operation"
        );
        let path = operation["path"].as_str();
        if path.is_none() && op != "remove" {
            let values = operation["value"]
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("object value required without path"))?;
            for (key, value) in values {
                apply(&mut resource, key, value, &op, user)?;
            }
        } else {
            apply(
                &mut resource,
                path.unwrap_or(""),
                &operation["value"],
                &op,
                user,
            )?;
        }
    }
    Ok(resource)
}
fn apply(
    resource: &mut Value,
    path: &str,
    value: &Value,
    op: &str,
    user: bool,
) -> anyhow::Result<()> {
    if !user && path.starts_with("members[value eq ") && path.ends_with(']') && op == "remove" {
        let id: String = serde_json::from_str(&path[17..path.len() - 1])?;
        if let Some(members) = resource["members"].as_array_mut() {
            members.retain(|m| m["value"] != id);
        }
        return Ok(());
    }
    let allowed = if user {
        &[
            "userName",
            "displayName",
            "name",
            "emails",
            "active",
            "externalId",
        ][..]
    } else {
        &["displayName", "members"][..]
    };
    ensure!(
        allowed.contains(&path),
        "unsupported or read-only patch attribute"
    );
    if op == "remove" {
        resource.as_object_mut().unwrap().remove(path);
    } else if path == "members" && op == "add" {
        let members = value
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("members must be array"))?;
        if resource.get("members").is_none() {
            resource["members"] = json!([]);
        }
        let existing = resource["members"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("invalid members"))?;
        for member in members {
            if !existing.iter().any(|m| m["value"] == member["value"]) {
                existing.push(member.clone());
            }
        }
    } else {
        resource[path] = value.clone();
    }
    Ok(())
}
pub(super) async fn delete(
    State(state): State<AppState>,
    Extension(store): Extension<Store>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response> {
    let user = users(&kind)?;
    store
        .update_directory(issuer(&state)?, &state.security.scim_group_roles, |d| {
            if resources_mut(d, user).remove(&id).is_none() {
                bail!("resource not found");
            }
            if user {
                for group in d.groups.values_mut() {
                    if let Some(members) = group["members"].as_array_mut() {
                        members.retain(|m| m["value"] != id);
                    }
                }
            }
            Ok(Value::Null)
        })
        .await
        .map_err(|e| {
            if e.to_string() == "resource not found" {
                missing()
            } else {
                bad(e)
            }
        })?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub(super) async fn configuration() -> Response {
    response(
        StatusCode::OK,
        json!({"schemas":["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],"patch":{"supported":true},"bulk":{"supported":false,"maxOperations":0,"maxPayloadSize":0},"filter":{"supported":true,"maxResults":1000},"changePassword":{"supported":false},"sort":{"supported":false},"etag":{"supported":false},"authenticationSchemes":[{"type":"oauthbearertoken","name":"Scoped admin bearer token","description":"Use a scoped Refract administrator key","primary":true}]}),
    )
}

#[cfg(test)]
mod tests;
