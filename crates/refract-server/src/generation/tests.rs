use super::*;
use crate::tests::{keys, request};
use std::sync::Mutex;
fn profile(endpoint: &str) -> Value {
    json!({"id":"test-model","label":"Test model","protocol":"openai_chat","endpoint":endpoint,"model":"candidate","scopes":[{"organization":"company","project":"a","environment":"test"}],"grading_rubric":"Preserve refund amount and eligibility."})
}
#[test]
fn operator_config_and_all_protocols_are_bounded() {
    assert!(
        Registry::parse(
            &json!([profile("http://localhost/model")]).to_string(),
            true
        )
        .is_err()
    );
    assert!(
        Registry::parse(
            &json!([profile("https://user:pass@example.invalid/model")]).to_string(),
            true
        )
        .is_err()
    );
    for protocol in [
        "openai_chat",
        "openai_responses",
        "anthropic",
        "gemini",
        "ollama",
        "custom",
    ] {
        let mut p = profile("https://fixture.invalid/model");
        p["protocol"] = protocol.into();
        p["request_template"] = json!({"payload":"$input","model":"$model"});
        p["response_pointer"] = "/text".into();
        let registry = Registry::parse(&json!([p]).to_string(), true).unwrap();
        let p = &registry.profiles["test-model"];
        let body=p.body(&json!({"input":"hello","api_key":"ignored","base_url":"https://attacker.invalid","max_tokens":9999999})).unwrap();
        if protocol != "custom" {
            assert!(!body.to_string().contains("attacker"));
            assert!(!body.to_string().contains("9999999"));
        }
        assert!(p.public().get("endpoint").is_none());
    }
}
#[tokio::test]
async fn replay_scope_authorization_model_grading_and_failure_closed() {
    type Calls = Arc<Mutex<Vec<Value>>>;
    let calls: Calls = Arc::default();
    let mock=Router::new().route("/model",post(|State(calls):State<Calls>,Json(body):Json<Value>|async move{
        calls.lock().unwrap().push(body.clone());
        let prompt=body["messages"][0]["content"].as_str().unwrap_or("");
        let content=if prompt.contains("bad-verdict") {"not json".into()}else if prompt.contains("rubric") {json!({"score":0.95,"equivalent":true,"reason":"Same eligibility and amount"}).to_string()}else{"fresh answer".into()};
        Json(json!({"choices":[{"message":{"role":"assistant","content":content}}],"usage":{"prompt_tokens":5,"completion_tokens":3}}))
    })).with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let registry = Registry::parse(
        &json!([profile(&format!(
            "http://{}/model",
            listener.local_addr().unwrap()
        ))])
        .to_string(),
        false,
    )
    .unwrap();
    let mock = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let store = Store::open("sqlite::memory:").await.unwrap();
    let app = router_with_policy(
        store,
        keys(),
        refract_collector::RedactionPolicy::default(),
        embeddings::Registry::default(),
        registry,
    );
    let writer = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let reader = Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr");
    let other = Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let mut run: Run = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/simple-run/execution.json"
    ))
    .unwrap();
    run.events[1].input = json!({"messages":[{"role":"user","content":"hello"}],"base_url":"https://attacker.invalid"});
    run.events[1].attributes = json!({"model":"original","total_tokens":999,"cost_usd":9});
    let path = format!("/v1/runs/{}/rerun", run.id);
    assert_eq!(
        request(&app, "POST", "/v1/runs", writer, json!(run))
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        request(&app, "GET", "/v1/generation-models", other, Value::Null)
            .await
            .1["models"],
        json!([])
    );
    let body = json!({"profile":"test-model","from_event":"evt_2","allow_live":true});
    assert_eq!(
        request(&app, "POST", &path, reader, body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "POST", &path, other, body.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            writer,
            json!({"profile":"test-model","from_event":"evt_2"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &path,
            writer,
            json!({"profile":"test-model","from_event":"evt_1","allow_live":true})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(calls.lock().unwrap().is_empty());
    let (status, branch) = request(&app, "POST", &path, writer, body).await;
    assert_eq!(status, StatusCode::CREATED, "{branch}");
    assert_eq!(branch["events"][1]["attributes"]["total_tokens"], 8);
    assert!(branch["events"][1]["attributes"].get("cost_usd").is_none());
    assert_ne!(branch["id"], run.id);
    assert_eq!(calls.lock().unwrap()[0]["model"], "candidate");
    let mut candidate = run.clone();
    candidate.id = "run_candidate".into();
    for e in &mut candidate.events {
        e.run_id = candidate.id.clone();
    }
    candidate.events[1].output = json!("same meaning");
    assert_eq!(
        request(&app, "POST", "/v1/runs", writer, json!(candidate))
            .await
            .0,
        StatusCode::CREATED
    );
    let mut diff =
        json!({"left":run.id,"right":candidate.id,"semantic":true,"grader":"test-model"});
    assert_eq!(
        request(&app, "POST", "/v1/diff", reader, diff.clone())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    diff["allow_live"] = true.into();
    let (status, report) = request(&app, "POST", "/v1/diff", reader, diff).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["semantic_report"]["passed"], true, "{report}");
    assert_eq!(
        report["semantic_report"]["differences"][0]["grade"]["grader"],
        "test-model:candidate"
    );
    candidate.id = "run_invalid".into();
    for e in &mut candidate.events {
        e.run_id = candidate.id.clone();
    }
    candidate.events[1].output = json!("bad-verdict");
    request(&app, "POST", "/v1/runs", writer, json!(candidate)).await;
    let(_,report)=request(&app,"POST","/v1/diff",reader,json!({"left":run.id,"right":candidate.id,"semantic":true,"grader":"test-model","allow_live":true})).await;
    assert_eq!(report["semantic_report"]["passed"], false);
    assert_eq!(
        report["semantic_report"]["differences"][0]["category"],
        "grader_error"
    );
    mock.abort();
}

#[tokio::test]
async fn provider_failures_are_bounded_without_redirects_or_retries() {
    type Count = Arc<std::sync::atomic::AtomicUsize>;
    use std::sync::atomic::Ordering;
    let count: Count = Arc::default();
    let mock = Router::new()
        .route(
            "/redirect",
            get(|| async { "unexpected" })
                .post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/target")]) }),
        )
        .route(
            "/target",
            post(|State(n): State<Count>| async move {
                n.fetch_add(1, Ordering::SeqCst);
                Json(json!({}))
            }),
        )
        .route(
            "/limited",
            post(|State(n): State<Count>| async move {
                n.fetch_add(1, Ordering::SeqCst);
                StatusCode::TOO_MANY_REQUESTS
            }),
        )
        .route(
            "/large",
            post(|| async { Json(json!({"text":"x".repeat(1024*1024)})) }),
        )
        .with_state(count.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    for path in ["redirect", "limited", "large"] {
        let registry = Registry::parse(
            &json!([profile(&format!("{base}/{path}"))]).to_string(),
            false,
        )
        .unwrap();
        assert!(
            registry
                .generate(&registry.profiles["test-model"], &json!({"input":"hello"}))
                .await
                .is_err()
        );
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "redirect target must never receive request; 429 must not retry"
    );
    task.abort();
}
