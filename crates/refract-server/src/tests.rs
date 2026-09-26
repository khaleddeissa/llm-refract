use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

fn fixture() -> Run {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/simple-run/execution.json"
    ))
    .unwrap()
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 20_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"body":String::from_utf8_lossy(&bytes)})),
    )
}
fn keys() -> Security {
    Security::from_keys(&json!([
        {"id":"writer-a","key":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","role":"writer","organization":"company","project":"a","environment":"test"},
        {"id":"reader-a","key":"rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr","role":"reader","organization":"company","project":"a","environment":"test"},
        {"id":"admin-a","key":"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz","role":"admin","organization":"company","project":"a","environment":"test"},
        {"id":"writer-b","key":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","role":"writer","organization":"company","project":"b","environment":"test"}
    ]).to_string()).unwrap()
}
#[tokio::test]
async fn ingest_replay_export_and_conflict() {
    let app = router(Store::open("sqlite::memory:").await.unwrap());
    for expected in [StatusCode::CREATED, StatusCode::CONFLICT] {
        assert_eq!(
            request(&app, "POST", "/v1/runs", None, json!(fixture()))
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/runs/demo-1/replay",
            None,
            json!({"mode":"live"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let response = app
        .oneshot(
            Request::get("/v1/runs/demo-1/artifact")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        refract_artifact::unpack(&to_bytes(response.into_body(), 1_000_000).await.unwrap())
            .unwrap()
            .events
            .len(),
        2
    );
}
#[tokio::test]
async fn tenants_roles_and_audit_protect_every_route() {
    let app = router_with_security(Store::open("sqlite::memory:").await.unwrap(), keys());
    let a = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let b = Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let reader = Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr");
    let admin = Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    assert_eq!(
        request(&app, "GET", "/v1/health", None, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", "/v1/runs", None, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "POST", "/v1/runs", a, json!(fixture()))
            .await
            .0,
        StatusCode::CREATED
    );
    for path in [
        "/v1/runs/demo-1",
        "/v1/runs/demo-1/events",
        "/v1/runs/demo-1/artifact",
        "/v1/runs/demo-1/metrics",
        "/v1/runs/demo-1/similar",
    ] {
        assert_eq!(
            request(&app, "GET", path, b, Value::Null).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    for (path, body) in [
        ("/v1/runs/demo-1/replay", json!({"mode":"exact"})),
        ("/v1/runs/demo-1/fork", json!({"from_event":"evt_1"})),
        ("/v1/diff", json!({"left":"demo-1","right":"demo-1"})),
        (
            "/v1/eval",
            json!({"pairs":[{"name":"test","left":"demo-1","right":"demo-1"}]}),
        ),
    ] {
        assert_eq!(
            request(&app, "POST", path, b, body).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    assert_eq!(
        request(&app, "GET", "/v1/search", b, Value::Null).await.1["total"],
        0
    );
    assert_eq!(
        request(&app, "GET", "/v1/runs", b, Value::Null).await.1,
        json!([])
    );
    assert_eq!(
        request(&app, "POST", "/v1/runs", reader, json!(fixture()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/runs/batch",
            reader,
            json!({"runs":[fixture()]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/runs/demo-1/fork",
            reader,
            json!({"from_event":"evt_1"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", "/v1/runs/demo-1", reader, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/diff",
            reader,
            json!({"left":"demo-1","right":"demo-1"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", "/v1/admin/audit", a, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, audit) = request(&app, "GET", "/v1/admin/audit", admin, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        audit["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["status"] == 403)
    );
    assert!(
        !audit
            .to_string()
            .contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert!(
        audit["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["actor"] != "writer-b")
    );
    // Identical run IDs are independent snapshots in each scope.
    let mut run = fixture();
    run.name = "other workspace".into();
    assert_eq!(
        request(&app, "POST", "/v1/runs", b, json!(run)).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        request(&app, "GET", "/v1/runs/demo-1", a, Value::Null)
            .await
            .1["name"],
        fixture().name
    );
}
#[tokio::test]
async fn batches_search_metrics_evaluation_and_rate_limit() {
    let app = router(Store::open("sqlite::memory:").await.unwrap());
    let mut run = fixture();
    run.events[1].attributes =
        json!({"model":"test-model","input_tokens":100,"output_tokens":20,"cost_usd":0.5});
    let body = json!({"runs":[run.clone(),run.clone()]});
    let (status, receipt) = request(&app, "POST", "/v1/runs/batch", None, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["accepted"], 1);
    assert_eq!(receipt["duplicates"], 1);
    assert_eq!(
        request(&app, "POST", "/v1/runs/batch", None, body).await.1["duplicates"],
        2
    );
    let mut conflict = run.clone();
    conflict.name = "conflict".into();
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/runs/batch",
            None,
            json!({"runs":[Run::new("must rollback"),conflict]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let page = request(
        &app,
        "GET",
        "/v1/search?model=test-model&min_cost_usd=0.4&limit=1",
        None,
        Value::Null,
    )
    .await
    .1;
    assert_eq!(page["total"], 1);
    assert_eq!(page["limit"], 1);
    assert_eq!(
        request(&app, "GET", "/v1/search?limit=0", None, Value::Null)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let metrics = request(&app, "GET", "/v1/runs/demo-1/metrics", None, Value::Null)
        .await
        .1;
    assert_eq!(metrics["input_tokens"], 100);
    assert_eq!(metrics["cost_usd"], 0.5);
    let diff = request(
        &app,
        "POST",
        "/v1/diff",
        None,
        json!({"left":"demo-1","right":"demo-1","semantic":true}),
    )
    .await
    .1;
    assert_eq!(diff["semantic_report"]["passed"], true);
    assert!(diff["metric_changes"].is_object());
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/diff",
            None,
            json!({"left":"demo-1","right":"demo-1","options":{"similarity_threshold":3}})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let eval = request(
        &app,
        "POST",
        "/v1/eval",
        None,
        json!({"pairs":[{"name":"identity","left":"demo-1","right":"demo-1"}]}),
    )
    .await
    .1;
    assert_eq!(eval["passed"], true);
    assert_eq!(eval["total"], 1);
    let similar = request(
        &app,
        "GET",
        "/v1/runs/demo-1/similar?limit=10",
        None,
        Value::Null,
    )
    .await
    .1;
    assert_eq!(similar["candidate_limit"], 1000);
    let mut security = keys();
    security.requests_per_minute = 1;
    let app = router_with_security(Store::open("sqlite::memory:").await.unwrap(), security);
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/runs",
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/runs",
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Value::Null
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/v1/runs",
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[test]
fn invalid_security_config_fails_closed() {
    assert!(Security::from_keys("[]").is_err());
    assert!(Security::from_keys(r#"[{"id":"a","key":"short","role":"admin","organization":"a","project":"a","environment":"a"}]"#).is_err());
    assert!(keys().authenticate(Some("incorrect")).is_none());
}

#[test]
fn production_profile_fails_closed_without_each_control() {
    assert!(security::validate_mode("local", &Security::default(), false, false).is_ok());
    assert!(security::validate_mode("typo", &keys(), true, true).is_err());
    assert!(security::validate_mode("production", &Security::default(), true, true).is_err());
    assert!(security::validate_mode("production", &keys(), false, true).is_err());
    assert!(security::validate_mode("production", &keys(), true, false).is_err());
    assert!(security::validate_mode("production", &keys(), true, true).is_ok());
}
#[tokio::test]
async fn tampered_credentials_cannot_override_scope_or_admin_role() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let app = router_with_security(store.clone(), keys());
    let a = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        request(&app, "POST", "/v1/runs", a, json!(fixture()))
            .await
            .0,
        StatusCode::CREATED
    );
    for key in [
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab",
        "",
        "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert_eq!(
            request(&app, "GET", "/v1/runs/demo-1", Some(key), Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::get("/v1/runs/demo-1")
                .header("authorization", "Bearer bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                .header("x-refract-project", "a")
                .header("x-refract-role", "admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        request(&app, "POST", "/v1/admin/retention", a, json!({"days":1}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    // Retention uses receipt time: an old trace ingested now must remain.
    let admin = Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/admin/retention",
            admin,
            json!({"days":1})
        )
        .await
        .1["deleted"],
        0
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/v1/admin/retention",
            admin,
            json!({"days":0})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, "GET", "/v1/admin/outbox", a, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", "/v1/admin/outbox", admin, Value::Null)
            .await
            .1["pending"],
        0
    );
}
