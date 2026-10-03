use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::logs_service_client::LogsServiceClient,
        metrics::v1::metrics_service_client::MetricsServiceClient,
    },
    common::v1::{AnyValue, any_value},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    metrics::v1::{
        Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric, number_data_point,
    },
};
use tower::ServiceExt;
fn private() -> KeyValue {
    KeyValue {
        key: "api_key".into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue("must-not-persist".into())),
        }),
        ..Default::default()
    }
}
fn log_document() -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            scope_logs: vec![ScopeLogs {
                log_records: vec![LogRecord {
                    time_unix_nano: 123,
                    trace_id: vec![1; 16],
                    span_id: vec![2; 8],
                    severity_text: "INFO".into(),
                    attributes: vec![private()],
                    body: Some(AnyValue {
                        value: Some(any_value::Value::StringValue("model completed".into())),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}
fn metric_document() -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            scope_metrics: vec![ScopeMetrics {
                metrics: vec![Metric {
                    name: "requests.active".into(),
                    data: Some(metric::Data::Gauge(Gauge {
                        data_points: vec![NumberDataPoint {
                            time_unix_nano: 123,
                            attributes: vec![private()],
                            value: Some(number_data_point::Value::AsInt(3)),
                            ..Default::default()
                        }],
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}
#[tokio::test]
async fn http_logs_metrics_redaction_retry_isolation_and_retention() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let app = router_with_security(store.clone(), crate::tests::keys());
    for (path, json, bytes) in [
        (
            "logs",
            serde_json::to_vec(&log_document()).unwrap(),
            log_document().encode_to_vec(),
        ),
        (
            "metrics",
            serde_json::to_vec(&metric_document()).unwrap(),
            metric_document().encode_to_vec(),
        ),
    ] {
        for (format, body) in [
            ("application/json", json),
            ("application/x-protobuf", bytes),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/v1/{path}"))
                        .header("content-type", format)
                        .header("authorization", "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{:?}",
                to_bytes(response.into_body(), MAX_BODY).await.unwrap()
            );
        }
        let (_, body) = crate::tests::request(
            &app,
            "GET",
            &format!("/v1/telemetry?kind={path}"),
            Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr"),
            Value::Null,
        )
        .await;
        assert_eq!(body["records"].as_array().unwrap().len(), 1);
        assert!(body.to_string().contains("[REDACTED]"));
        assert!(!body.to_string().contains("must-not-persist"));
        let (_, other) = crate::tests::request(
            &app,
            "GET",
            &format!("/v1/telemetry?kind={path}"),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            Value::Null,
        )
        .await;
        assert_eq!(other["records"], json!([]));
    }
    assert_eq!(
        crate::tests::request(
            &app,
            "POST",
            "/v1/logs",
            Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr"),
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let scoped = store.scoped(refract_storage::Scope {
        organization: "company".into(),
        project: "a".into(),
        environment: "test".into(),
    });
    assert_eq!(
        scoped
            .expire_telemetry(chrono::Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap(),
        2
    );
    assert!(
        scoped
            .telemetry("logs", "", 10, 0)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn native_grpc_logs_and_metrics_clients_require_auth() {
    let app = router_with_security(
        Store::open("sqlite::memory:").await.unwrap(),
        crate::tests::keys(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut logs = LogsServiceClient::connect(address.clone()).await.unwrap();
    let mut metrics = MetricsServiceClient::connect(address).await.unwrap();
    assert!(logs.export(log_document()).await.is_err());
    assert!(metrics.export(metric_document()).await.is_err());
    let mut request = tonic::Request::new(log_document());
    request.metadata_mut().insert(
        "authorization",
        "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
    );
    logs.export(request).await.unwrap();
    let mut request = tonic::Request::new(metric_document());
    request.metadata_mut().insert(
        "authorization",
        "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
    );
    metrics.export(request).await.unwrap();
    task.abort();
}

#[tokio::test]
async fn standard_otlp_json_preserves_string_integers_and_base64_bytes() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let app = router(store.clone());
    let metric = json!({"resourceMetrics":[{"scopeMetrics":[{"metrics":[{"name":"requests","sum":{"aggregationTemporality":2,"isMonotonic":true,"dataPoints":[{"timeUnixNano":"123","asInt":"9223372036854775807","attributes":[{"key":"attempt","value":{"intValue":"3"}}]}]}}]}]}]});
    assert_eq!(
        crate::tests::request(&app, "POST", "/v1/metrics", None, metric)
            .await
            .0,
        StatusCode::OK
    );
    let records = store.telemetry("metrics", "", 10, 0).await.unwrap();
    assert_eq!(
        records[0].payload["metric"]["sum"]["dataPoints"][0]["asInt"],
        json!(i64::MAX)
    );
    assert_eq!(
        records[0].payload["metric"]["sum"]["dataPoints"][0]["attributes"]["attempt"],
        3
    );
    let logs = json!({"resourceLogs":[{"scopeLogs":[{"logRecords":[{"timeUnixNano":"123","body":{"bytesValue":"aGVsbG8="}}]}]}]});
    assert_eq!(
        crate::tests::request(&app, "POST", "/v1/logs", None, logs)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        store.telemetry("logs", "", 10, 0).await.unwrap()[0].payload["body"],
        "68656c6c6f"
    );
}
