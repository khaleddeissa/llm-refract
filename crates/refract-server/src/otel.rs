//! Native OTLP transport and durable, tenant-scoped distributed trace assembly.
use super::*;
use anyhow::{Result, anyhow, ensure};
use axum::{body::Bytes, http::HeaderMap};
use chrono::{DateTime, Utc};
use opentelemetry_proto::tonic::{
    collector::trace::v1::{
        ExportTraceServiceRequest, ExportTraceServiceResponse,
        trace_service_server::{TraceService, TraceServiceServer},
    },
    common::v1::{AnyValue, KeyValue, any_value},
    trace::v1::Span,
};
use prost::Message;
use refract_core::{Event, EventType, ReplayPolicy, Status};
use refract_storage::TraceSpan;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, VecDeque};

const MAX_BODY: usize = 16 * 1024 * 1024;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(super) fn value(item: &AnyValue) -> Value {
    match &item.value {
        Some(any_value::Value::StringValue(v)) => json!(v),
        Some(any_value::Value::IntValue(v)) => json!(v),
        Some(any_value::Value::BoolValue(v)) => json!(v),
        Some(any_value::Value::DoubleValue(v)) => json!(v),
        Some(any_value::Value::ArrayValue(v)) => Value::Array(v.values.iter().map(value).collect()),
        Some(any_value::Value::KvlistValue(v)) => attributes(&v.values),
        Some(any_value::Value::BytesValue(v)) => json!(hex(v)),
        _ => Value::Null,
    }
}
pub(super) fn attributes(items: &[KeyValue]) -> Value {
    Value::Object(
        items
            .iter()
            .map(|v| {
                (
                    v.key.clone(),
                    v.value.as_ref().map(value).unwrap_or(Value::Null),
                )
            })
            .collect(),
    )
}
fn timestamp(nanos: u64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp(
        (nanos / 1_000_000_000).try_into()?,
        (nanos % 1_000_000_000) as u32,
    )
    .ok_or_else(|| anyhow!("invalid span timestamp"))
}
fn field(attrs: &Value, key: &str) -> Result<Value> {
    match attrs.get(key) {
        Some(Value::String(value)) => Ok(serde_json::from_str(value)?),
        Some(value) => Ok(value.clone()),
        None => Ok(Value::Null),
    }
}
fn span(
    span: Span,
    resource: &Value,
    policy: &refract_collector::RedactionPolicy,
) -> Result<(String, TraceSpan)> {
    ensure!(
        span.trace_id.len() == 16 && span.trace_id.iter().any(|b| *b != 0),
        "invalid trace id"
    );
    ensure!(
        span.span_id.len() == 8 && span.span_id.iter().any(|b| *b != 0),
        "invalid span id"
    );
    ensure!(
        span.parent_span_id.is_empty() || span.parent_span_id.len() == 8,
        "invalid parent id"
    );
    ensure!(
        span.span_id != span.parent_span_id,
        "span cannot parent itself"
    );
    ensure!(
        span.end_time_unix_nano >= span.start_time_unix_nano,
        "span ends before it starts"
    );
    let attrs = attributes(&span.attributes);
    let mut canonical = field(&attrs, "refract.event.attributes")?;
    if canonical.is_null() {
        canonical = json!({});
    }
    ensure!(canonical.is_object(), "event attributes must be an object");
    for (from, to) in [
        ("gen_ai.request.model", "model"),
        ("gen_ai.system", "provider"),
        ("gen_ai.provider.name", "provider"),
        ("gen_ai.usage.input_tokens", "input_tokens"),
        ("gen_ai.usage.output_tokens", "output_tokens"),
    ] {
        if let Some(value) = attrs.get(from) {
            canonical[to] = value.clone();
        }
    }
    canonical["otel.attributes"] = attrs.clone();
    canonical["otel.resource"] = resource.clone();
    canonical["otel.links"] = serde_json::to_value(span.links)?;
    canonical["otel.events"] = serde_json::to_value(span.events)?;
    let trace_id = hex(&span.trace_id);
    let span_id = hex(&span.span_id);
    let parent = (!span.parent_span_id.is_empty()).then(|| hex(&span.parent_span_id));
    let kind = match attrs.get("refract.event.type") {
        Some(v) => serde_json::from_value(v.clone())?,
        None => {
            if canonical.get("model").is_some() {
                EventType::Generation
            } else {
                EventType::Decision
            }
        }
    };
    let start = timestamp(span.start_time_unix_nano)?;
    let end = timestamp(span.end_time_unix_nano)?;
    let mut run = Run::new("OTLP staging");
    run.id = "otel-staging".into();
    run.started_at = start;
    run.ended_at = Some(end);
    run.events.push(Event {
        id: format!("otel_{span_id}"),
        run_id: run.id.clone(),
        parent_id: None,
        kind,
        name: if span.name.trim().is_empty() {
            "OTel span".into()
        } else {
            span.name
        },
        timestamp: start,
        duration_ms: (span.end_time_unix_nano - span.start_time_unix_nano) as f64 / 1_000_000.0,
        status: if span.status.is_some_and(|s| s.code == 2) {
            Status::Failed
        } else {
            Status::Completed
        },
        input: field(&attrs, "refract.event.input")?,
        output: field(&attrs, "refract.event.output")?,
        attributes: canonical,
        replay_policy: ReplayPolicy::Recorded,
    });
    let mut normalized = policy.normalize(run)?;
    Ok((
        trace_id,
        TraceSpan {
            id: span_id,
            parent,
            event: normalized.events.remove(0),
            ended_at: end,
        },
    ))
}
fn assemble(trace_id: &str, spans: Vec<TraceSpan>) -> Result<Run> {
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&spans)?));
    let id = format!("otel_{trace_id}_{}", &digest[..16]);
    let known: HashMap<_, _> = spans
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), i))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut ready = VecDeque::new();
    for (i, span) in spans.iter().enumerate() {
        match span.parent.as_ref().and_then(|p| known.get(p)) {
            Some(parent) => children.entry(*parent).or_default().push(i),
            None => ready.push_back(i),
        }
    }
    let mut run = Run::new(format!("OTel trace {trace_id}"));
    run.id = id;
    run.started_at = spans
        .iter()
        .map(|s| s.event.timestamp)
        .min()
        .ok_or_else(|| anyhow!("empty trace"))?;
    run.ended_at = spans.iter().map(|s| s.ended_at).max();
    run.status = Status::Completed;
    run.metadata = json!({"otel.trace_id":trace_id,"otel.snapshot_digest":digest,"otel.span_count":spans.len()});
    while let Some(index) = ready.pop_front() {
        let source = &spans[index];
        let mut event = source.event.clone();
        event.run_id = run.id.clone();
        if let Some(parent) = &source.parent {
            if known.contains_key(parent) {
                event.parent_id = Some(format!("otel_{parent}"));
            } else {
                event.attributes["otel.external_parent_id"] = json!(parent);
            }
        }
        if event.status == Status::Failed {
            run.status = Status::Failed;
        }
        run.events.push(event);
        ready.extend(children.remove(&index).unwrap_or_default());
    }
    ensure!(
        run.events.len() == spans.len(),
        "trace contains a parent cycle"
    );
    run.validate()?;
    Ok(run)
}
async fn ingest(
    store: &Store,
    policy: &refract_collector::RedactionPolicy,
    request: ExportTraceServiceRequest,
) -> ApiResult<()> {
    let count: usize = request
        .resource_spans
        .iter()
        .flat_map(|r| &r.scope_spans)
        .map(|s| s.spans.len())
        .sum();
    if count > 1000 {
        return Err(invalid("OTLP batch exceeds 1000 spans"));
    }
    let mut traces: BTreeMap<String, Vec<TraceSpan>> = BTreeMap::new();
    for resource in request.resource_spans {
        let attrs = resource
            .resource
            .map(|r| attributes(&r.attributes))
            .unwrap_or(json!({}));
        for scope in resource.scope_spans {
            for item in scope.spans {
                let (trace, normalized) = span(item, &attrs, policy).map_err(invalid)?;
                traces.entry(trace).or_default().push(normalized);
            }
        }
    }
    for (trace, spans) in traces {
        let all = store.merge_trace(&trace, &spans).await.map_err(internal)?;
        let run = assemble(&trace, all).map_err(invalid)?;
        store.insert_batch(&[run]).await.map_err(internal)?;
    }
    Ok(())
}
pub(super) async fn http(
    Extension(store): Extension<Store>,
    Extension(policy): Extension<refract_collector::RedactionPolicy>,
    headers: HeaderMap,
    bytes: Bytes,
) -> ApiResult<Response> {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("");
    let request = match content_type {
        "application/x-protobuf" => ExportTraceServiceRequest::decode(bytes).map_err(invalid)?,
        "application/json" => serde_json::from_slice(&bytes).map_err(invalid)?,
        _ => {
            return Err(ApiError(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "use OTLP JSON or protobuf".into(),
            ));
        }
    };
    ingest(&store, &policy, request).await?;
    if content_type == "application/json" {
        Ok(Json(json!({})).into_response())
    } else {
        Ok((
            [("content-type", "application/x-protobuf")],
            ExportTraceServiceResponse::default().encode_to_vec(),
        )
            .into_response())
    }
}
#[derive(Default)]
pub(super) struct Collector;
#[tonic::async_trait]
impl TraceService for Collector {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        let store = request
            .extensions()
            .get::<Store>()
            .cloned()
            .ok_or_else(|| tonic::Status::unauthenticated("authentication required"))?;
        let policy = request
            .extensions()
            .get::<refract_collector::RedactionPolicy>()
            .cloned()
            .ok_or_else(|| tonic::Status::internal("redaction policy unavailable"))?;
        ingest(&store, &policy, request.into_inner())
            .await
            .map_err(|_| tonic::Status::invalid_argument("trace ingestion failed"))?;
        Ok(tonic::Response::new(ExportTraceServiceResponse::default()))
    }
}
pub(super) fn grpc() -> TraceServiceServer<Collector> {
    TraceServiceServer::new(Collector).max_decoding_message_size(MAX_BODY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::{
        collector::trace::v1::trace_service_client::TraceServiceClient,
        trace::v1::{ResourceSpans, ScopeSpans},
    };
    fn document(child: bool) -> ExportTraceServiceRequest {
        ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                scope_spans: vec![ScopeSpans {
                    spans: vec![Span {
                        trace_id: vec![1; 16],
                        span_id: vec![if child { 2 } else { 1 }; 8],
                        parent_span_id: if child { vec![1; 8] } else { vec![] },
                        name: if child {
                            "generation".into()
                        } else {
                            "root".into()
                        },
                        start_time_unix_nano: 1_000_000_000,
                        end_time_unix_nano: 2_000_000_000,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }
    #[tokio::test]
    async fn separate_batches_assemble_parents_and_retries_are_idempotent() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let policy = refract_collector::RedactionPolicy::default();
        ingest(&store, &policy, document(true))
            .await
            .unwrap_or_else(|_| panic!("child ingest"));
        ingest(&store, &policy, document(false))
            .await
            .unwrap_or_else(|_| panic!("parent ingest"));
        ingest(&store, &policy, document(true))
            .await
            .unwrap_or_else(|_| panic!("retry ingest"));
        let runs = store.list().await.unwrap();
        assert_eq!(runs.len(), 2);
        let merged = runs.iter().find(|run| run.events.len() == 2).unwrap();
        assert_eq!(
            merged.events[1].parent_id.as_ref(),
            Some(&merged.events[0].id)
        );
        let other = store.scoped(refract_storage::Scope {
            project: "other".into(),
            ..Default::default()
        });
        assert!(other.list().await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn native_grpc_client_exports_through_authenticated_axum_router() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let app = router_with_security(store.clone(), crate::tests::keys());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut client = TraceServiceClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        assert!(client.export(document(false)).await.is_err());
        let mut req = tonic::Request::new(document(false));
        req.metadata_mut().insert(
            "authorization",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
        );
        client.export(req).await.unwrap();
        let scoped = store.scoped(refract_storage::Scope {
            organization: "company".into(),
            project: "a".into(),
            environment: "test".into(),
        });
        assert_eq!(scoped.list().await.unwrap().len(), 1);
        server.abort();
    }
    #[tokio::test]
    async fn http_json_and_protobuf_use_the_same_normalizer() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let store = Store::open("sqlite::memory:").await.unwrap();
        let app = router(store.clone());
        for (content_type, body) in [
            ("application/x-protobuf", document(false).encode_to_vec()),
            (
                "application/json",
                serde_json::to_vec(&document(true)).unwrap(),
            ),
        ] {
            let request = Request::post("/v1/traces")
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
        assert!(
            store
                .list()
                .await
                .unwrap()
                .iter()
                .any(|r| r.events.len() == 2)
        );
    }
}

#[cfg(test)]
mod cycle_tests {
    use super::*;
    #[tokio::test]
    async fn cyclic_late_span_rolls_back_without_poisoning_the_trace() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let mut run = Run::new("span");
        run.events.push(Event {
            id: "a".into(),
            run_id: run.id.clone(),
            parent_id: None,
            kind: EventType::Decision,
            name: "span".into(),
            timestamp: run.started_at,
            duration_ms: 1.0,
            status: Status::Completed,
            input: Value::Null,
            output: Value::Null,
            attributes: json!({}),
            replay_policy: ReplayPolicy::Recorded,
        });
        let a = TraceSpan {
            id: "a".into(),
            parent: Some("b".into()),
            event: run.events[0].clone(),
            ended_at: run.started_at,
        };
        let trace = "a".repeat(32);
        store
            .merge_trace(&trace, std::slice::from_ref(&a))
            .await
            .unwrap();
        let mut b = a.clone();
        b.id = "b".into();
        b.parent = Some("a".into());
        assert!(store.merge_trace(&trace, &[b.clone()]).await.is_err());
        b.parent = None;
        assert_eq!(store.merge_trace(&trace, &[b]).await.unwrap().len(), 2);
    }
}
