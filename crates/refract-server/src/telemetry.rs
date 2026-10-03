//! OTLP metrics and logs share the trace receiver's authenticated, scoped transports.
use super::*;
use axum::{body::Bytes, http::HeaderMap};
use opentelemetry_proto::tonic::{
    collector::logs::v1::{
        ExportLogsServiceRequest, ExportLogsServiceResponse,
        logs_service_server::{LogsService, LogsServiceServer},
    },
    collector::metrics::v1::{
        ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        metrics_service_server::{MetricsService, MetricsServiceServer},
    },
    common::v1::KeyValue,
};
use prost::Message;
use refract_collector::RedactionPolicy;
use refract_storage::TelemetryRecord;
const MAX_BODY: usize = 16 * 1024 * 1024;

fn normalize(policy: &RedactionPolicy, payload: Value) -> ApiResult<Value> {
    let mut wrapper = Run::new("OTLP telemetry");
    wrapper.metadata = payload;
    Ok(policy.normalize(wrapper).map_err(invalid)?.metadata)
}
/// OTLP represents attributes as [{key,value}]; convert those lists before key-based redaction.
fn flatten_attributes(value: &mut Value) -> ApiResult<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "attributes" | "filteredAttributes" | "filtered_attributes"
                ) && value.is_array()
                {
                    let attributes: Vec<KeyValue> =
                        serde_json::from_value(value.clone()).map_err(invalid)?;
                    *value = otel::attributes(&attributes);
                } else {
                    flatten_attributes(value)?;
                }
            }
        }
        Value::Array(array) => {
            for value in array {
                flatten_attributes(value)?;
            }
        }
        _ => (),
    }
    Ok(())
}
async fn logs(
    store: &Store,
    policy: &RedactionPolicy,
    request: ExportLogsServiceRequest,
) -> ApiResult<()> {
    let mut records = Vec::new();
    for resource in request.resource_logs {
        let resource_data = resource
            .resource
            .map(|r| otel::attributes(&r.attributes))
            .unwrap_or_else(|| json!({}));
        for scope in resource.scope_logs {
            let mut instrumentation = serde_json::to_value(scope.scope).map_err(invalid)?;
            flatten_attributes(&mut instrumentation)?;
            for record in scope.log_records {
                if records.len() >= 10000 {
                    return Err(invalid("at most 10000 log records per export"));
                }
                if (!record.trace_id.is_empty()
                    && (record.trace_id.len() != 16 || record.trace_id.iter().all(|b| *b == 0)))
                    || (!record.span_id.is_empty()
                        && (record.span_id.len() != 8 || record.span_id.iter().all(|b| *b == 0)))
                {
                    return Err(invalid("invalid log trace or span id"));
                }
                let trace_id = record
                    .trace_id
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                let span_id = record
                    .span_id
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                let payload = json!({"resource":resource_data,"scope":instrumentation,"schema_url":scope.schema_url,"time_unix_nano":record.time_unix_nano.to_string(),"observed_time_unix_nano":record.observed_time_unix_nano.to_string(),"severity_number":record.severity_number,"severity_text":record.severity_text,"trace_id":trace_id,"span_id":span_id,"body":record.body.as_ref().map(otel::value),"attributes":otel::attributes(&record.attributes),"dropped_attributes_count":record.dropped_attributes_count,"flags":record.flags});
                records.push(TelemetryRecord {
                    kind: "logs".into(),
                    trace_id,
                    payload: normalize(policy, payload)?,
                });
            }
        }
    }
    store.insert_telemetry(&records).await.map_err(invalid)?;
    Ok(())
}
async fn metrics(
    store: &Store,
    policy: &RedactionPolicy,
    request: ExportMetricsServiceRequest,
) -> ApiResult<()> {
    let mut records = Vec::new();
    for resource in request.resource_metrics {
        let resource_data = resource
            .resource
            .map(|r| otel::attributes(&r.attributes))
            .unwrap_or_else(|| json!({}));
        for scope in resource.scope_metrics {
            let mut instrumentation = serde_json::to_value(scope.scope).map_err(invalid)?;
            flatten_attributes(&mut instrumentation)?;
            for metric in scope.metrics {
                if records.len() >= 10000 || metric.name.is_empty() || metric.data.is_none() {
                    return Err(invalid("invalid metric or export exceeds 10000 metrics"));
                }
                let mut data = serde_json::to_value(metric).map_err(invalid)?;
                flatten_attributes(&mut data)?;
                let payload = json!({"resource":resource_data,"scope":instrumentation,"schema_url":scope.schema_url,"metric":data});
                records.push(TelemetryRecord {
                    kind: "metrics".into(),
                    trace_id: String::new(),
                    payload: normalize(policy, payload)?,
                });
            }
        }
    }
    store.insert_telemetry(&records).await.map_err(invalid)?;
    Ok(())
}
fn decode<T: Message + Default + serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    bytes: Bytes,
) -> ApiResult<(T, bool)> {
    if bytes.len() > MAX_BODY {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "OTLP export exceeds 16 MiB".into(),
        ));
    }
    match headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
    {
        "application/json" => Ok((otel::decode_json(&bytes)?, true)),
        "application/x-protobuf" => Ok((T::decode(bytes).map_err(invalid)?, false)),
        _ => Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "use OTLP JSON or protobuf".into(),
        )),
    }
}
pub(super) async fn logs_http(
    Extension(store): Extension<Store>,
    Extension(policy): Extension<RedactionPolicy>,
    headers: HeaderMap,
    bytes: Bytes,
) -> ApiResult<Response> {
    let (request, json) = decode(&headers, bytes)?;
    logs(&store, &policy, request).await?;
    Ok(if json {
        Json(json!({})).into_response()
    } else {
        (
            [("content-type", "application/x-protobuf")],
            ExportLogsServiceResponse::default().encode_to_vec(),
        )
            .into_response()
    })
}
pub(super) async fn metrics_http(
    Extension(store): Extension<Store>,
    Extension(policy): Extension<RedactionPolicy>,
    headers: HeaderMap,
    bytes: Bytes,
) -> ApiResult<Response> {
    let (request, json) = decode(&headers, bytes)?;
    metrics(&store, &policy, request).await?;
    Ok(if json {
        Json(json!({})).into_response()
    } else {
        (
            [("content-type", "application/x-protobuf")],
            ExportMetricsServiceResponse::default().encode_to_vec(),
        )
            .into_response()
    })
}
#[derive(Default)]
pub(super) struct Collector;
fn context<T>(request: &tonic::Request<T>) -> Result<(Store, RedactionPolicy), tonic::Status> {
    Ok((
        request
            .extensions()
            .get::<Store>()
            .cloned()
            .ok_or_else(|| tonic::Status::unauthenticated("authentication required"))?,
        request
            .extensions()
            .get::<RedactionPolicy>()
            .cloned()
            .ok_or_else(|| tonic::Status::internal("redaction unavailable"))?,
    ))
}
#[tonic::async_trait]
impl LogsService for Collector {
    async fn export(
        &self,
        request: tonic::Request<ExportLogsServiceRequest>,
    ) -> Result<tonic::Response<ExportLogsServiceResponse>, tonic::Status> {
        let (store, policy) = context(&request)?;
        logs(&store, &policy, request.into_inner())
            .await
            .map_err(|_| tonic::Status::invalid_argument("log ingestion failed"))?;
        Ok(tonic::Response::new(ExportLogsServiceResponse::default()))
    }
}
#[tonic::async_trait]
impl MetricsService for Collector {
    async fn export(
        &self,
        request: tonic::Request<ExportMetricsServiceRequest>,
    ) -> Result<tonic::Response<ExportMetricsServiceResponse>, tonic::Status> {
        let (store, policy) = context(&request)?;
        metrics(&store, &policy, request.into_inner())
            .await
            .map_err(|_| tonic::Status::invalid_argument("metric ingestion failed"))?;
        Ok(tonic::Response::new(ExportMetricsServiceResponse::default()))
    }
}
pub(super) fn logs_grpc() -> LogsServiceServer<Collector> {
    LogsServiceServer::new(Collector).max_decoding_message_size(MAX_BODY)
}
pub(super) fn metrics_grpc() -> MetricsServiceServer<Collector> {
    MetricsServiceServer::new(Collector).max_decoding_message_size(MAX_BODY)
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Filter {
    kind: String,
    trace_id: String,
    limit: i64,
    offset: i64,
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            kind: "logs".into(),
            trace_id: String::new(),
            limit: 50,
            offset: 0,
        }
    }
}
pub(super) async fn list(
    Extension(store): Extension<Store>,
    Query(filter): Query<Filter>,
) -> ApiResult<Json<Value>> {
    let records = store
        .telemetry(&filter.kind, &filter.trace_id, filter.limit, filter.offset)
        .await
        .map_err(invalid)?;
    Ok(Json(
        json!({"kind":filter.kind,"next_offset":filter.offset+records.len() as i64,"records":records}),
    ))
}

#[cfg(test)]
mod tests;
