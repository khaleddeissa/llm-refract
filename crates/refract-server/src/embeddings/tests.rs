use super::*;
use crate::tests::{keys, request};
use axum::{http::HeaderMap, routing::post};
use std::sync::Mutex;

fn profile(protocol: &str, endpoint: &str) -> Value {
    json!({"id":"local-embedding","label":"Local semantic model","protocol":protocol,"endpoint":endpoint,"model":"fixture-v1","dimensions":2})
}

#[test]
fn formats_namespaces_and_operator_config_are_checked() {
    for (protocol, pointer) in [
        ("openai", "/data/0/embedding"),
        ("voyage", "/data/0/embedding"),
        ("ollama", "/embeddings/0"),
        ("cohere", "/embeddings/float/0"),
        ("gemini", "/embedding/values"),
        ("vertex", "/predictions/0/embeddings/values"),
    ] {
        let p: Profile =
            serde_json::from_value(profile(protocol, "https://example.invalid/embed")).unwrap();
        assert_eq!(p.pointer(), pointer);
        let body = p.body("a quote: \" and newline\n", true);
        assert!(body.to_string().contains("quote"));
        if protocol == "voyage" {
            assert_eq!(body["input_type"], "query");
            assert_eq!(p.body("x", false)["input_type"], "document");
        }
        if protocol == "gemini" {
            assert_eq!(body["taskType"], "RETRIEVAL_QUERY");
        }
        if protocol == "cohere" {
            assert_eq!(body["input_type"], "search_query");
        }
        let mut changed = p.clone();
        changed.dimensions = 3;
        assert_ne!(p.namespace(), changed.namespace());
        changed = p.clone();
        changed.credential_env = Some("ROTATED_KEY".into());
        assert_eq!(p.namespace(), changed.namespace());
        assert!(p.public().get("endpoint").is_none());
        assert!(p.public().get("credential_env").is_none());
    }
    assert!(
        Registry::parse(
            &json!([profile("openai", "http://localhost/embed")]).to_string(),
            true
        )
        .is_err()
    );
    assert!(
        Registry::parse(
            &json!([profile("openai", "https://key@example.invalid/embed")]).to_string(),
            false
        )
        .is_err()
    );
    let mut custom = profile("custom", "https://example.invalid/embed");
    custom["request_template"] = json!({"text":"$text","model":"$model","purpose":"$task"});
    custom["response_pointer"] = "/vector".into();
    let registry = Registry::parse(&json!([custom]).to_string(), true).unwrap();
    assert_eq!(
        registry
            .profile("local-embedding", &Scope::default())
            .unwrap()
            .body("hello", false),
        json!({"text":"hello","model":"fixture-v1","purpose":"document"})
    );
}

#[tokio::test]
async fn local_provider_indexes_searches_and_enforces_scope_and_roles() {
    type Requests = Arc<Mutex<Vec<Value>>>;
    let seen: Requests = Arc::default();
    let mock = Router::new().route("/embed",post(|State(seen):State<Requests>, headers:HeaderMap, Json(body):Json<Value>| async move {
        assert!(headers.get("authorization").is_none());
        assert_eq!(body["model"],"fixture-v1");
        assert_eq!(body["encoding_format"],"float");
        seen.lock().unwrap().push(body.clone());
        let text=body["input"][0].as_str().unwrap();
        Json(json!({"data":[{"embedding":if text.contains("refund") {vec![1.0,0.0]} else {vec![0.0,1.0]}}]}))
    })).with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let registry = Registry::parse(
        &json!([profile(
            "openai",
            &format!("http://{}/embed", listener.local_addr().unwrap())
        )])
        .to_string(),
        false,
    )
    .unwrap();
    let mock = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let store = Store::open("sqlite::memory:").await.unwrap();
    let app = router_with_policy(
        store.clone(),
        keys(),
        refract_collector::RedactionPolicy::default(),
        registry.clone(),
        generation::Registry::default(),
    );
    let admin = Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    let writer = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let reader = Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr");
    let other = Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let selection = json!([{"profile":"local-embedding","is_default":true,"auto_index":true}]);
    assert_eq!(
        request(
            &app,
            "PUT",
            "/v1/admin/project/embeddings",
            writer,
            selection.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/v1/admin/project/embeddings",
            admin,
            selection
        )
        .await
        .0,
        StatusCode::OK
    );
    for name in ["refund for broken order", "change email address"] {
        let run = Run::new(name);
        assert_eq!(
            request(&app, "POST", "/v1/runs", writer, json!(run))
                .await
                .0,
            StatusCode::CREATED
        );
    }
    assert!(registry.process_one(&store).await.unwrap());
    assert!(registry.process_one(&store).await.unwrap());
    assert!(!registry.process_one(&store).await.unwrap());
    let query = json!({"query":"need a refund","limit":1});
    let (status, result) = request(&app, "POST", "/v1/search/text", reader, query.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["runs"][0]["name"], "refund for broken order");
    assert_eq!(result["matches"][0]["score"], 1.0);
    assert_eq!(
        request(&app, "POST", "/v1/search/text", other, query)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(seen.lock().unwrap().len(), 3);
    let (_, settings) = request(&app, "GET", "/v1/project/embeddings", reader, Value::Null).await;
    assert_eq!(settings["jobs"]["done"], 2);
    mock.abort();
}

#[tokio::test]
async fn invalid_provider_vectors_are_retried_without_false_completion() {
    let mock = Router::new().route(
        "/embed",
        post(|| async { Json(json!({"data":[{"embedding":[1.0]}]})) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let registry = Registry::parse(
        &json!([profile(
            "openai",
            &format!("http://{}/embed", listener.local_addr().unwrap())
        )])
        .to_string(),
        false,
    )
    .unwrap();
    let mock = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let store = Store::open("sqlite::memory:").await.unwrap();
    store
        .set_embedding_settings(&[EmbeddingSetting {
            profile: "local-embedding".into(),
            model: registry.profiles["local-embedding"].namespace(),
            is_default: true,
            auto_index: true,
        }])
        .await
        .unwrap();
    store.insert(&Run::new("invalid output")).await.unwrap();
    assert!(registry.process_one(&store).await.unwrap());
    assert_eq!(store.embedding_job_counts().await.unwrap()["pending"], 1);
    assert_eq!(store.embedding_job_counts().await.unwrap()["done"], 0);
    assert!(!registry.process_one(&store).await.unwrap());
    mock.abort();
}
