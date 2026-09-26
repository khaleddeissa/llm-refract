//! Provider-neutral measurements. Missing prices/usage remain unknown, never fabricated as zero.
use crate::{EventType, Run, Status};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub events: usize,
    pub model_calls: usize,
    pub tool_calls: usize,
    pub failures: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub measured_model_calls: usize,
    pub priced_model_calls: usize,
    pub cost_usd: Option<f64>,
    pub wall_time_ms: Option<f64>,
    pub event_duration_ms: f64,
    pub mean_ttft_ms: Option<f64>,
    pub slowest_event: Option<String>,
    pub most_expensive_event: Option<String>,
}

pub fn metrics(run: &Run) -> Metrics {
    let mut result = Metrics {
        events: run.events.len(),
        wall_time_ms: run
            .ended_at
            .map(|end| (end - run.started_at).num_microseconds().unwrap_or(0) as f64 / 1000.0),
        ..Metrics::default()
    };
    let mut ttft = Vec::new();
    let mut slowest = -1.0;
    let mut expensive = -1.0;
    for event in &run.events {
        result.model_calls += usize::from(event.kind == EventType::Generation);
        result.tool_calls += usize::from(event.kind == EventType::ToolCall);
        result.failures += usize::from(event.status == Status::Failed);
        result.event_duration_ms += event.duration_ms;
        if event.duration_ms > slowest {
            slowest = event.duration_ms;
            result.slowest_event = Some(event.id.clone());
        }
        let attrs = &event.attributes;
        if event.kind == EventType::Generation {
            if attrs["input_tokens"].as_u64().is_some() && attrs["output_tokens"].as_u64().is_some()
            {
                result.measured_model_calls += 1;
            }
            result.input_tokens = result
                .input_tokens
                .saturating_add(attrs["input_tokens"].as_u64().unwrap_or(0));
            result.output_tokens = result
                .output_tokens
                .saturating_add(attrs["output_tokens"].as_u64().unwrap_or(0));
            result.total_tokens =
                result
                    .total_tokens
                    .saturating_add(attrs["total_tokens"].as_u64().unwrap_or_else(|| {
                        attrs["input_tokens"]
                            .as_u64()
                            .unwrap_or(0)
                            .saturating_add(attrs["output_tokens"].as_u64().unwrap_or(0))
                    }));
            result.cache_write_tokens = result
                .cache_write_tokens
                .saturating_add(attrs["cache_write_tokens"].as_u64().unwrap_or(0));
            result.cache_read_tokens = result
                .cache_read_tokens
                .saturating_add(attrs["cache_read_tokens"].as_u64().unwrap_or(0));
            if let Some(value) = attrs["cost_usd"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
            {
                result.cost_usd = Some(result.cost_usd.unwrap_or(0.0) + value);
                result.priced_model_calls += 1;
                if value > expensive {
                    expensive = value;
                    result.most_expensive_event = Some(event.id.clone());
                }
            }
            if let Some(value) = attrs["ttft_ms"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
            {
                ttft.push(value);
            }
        }
    }
    if !ttft.is_empty() {
        result.mean_ttft_ms = Some(ttft.iter().sum::<f64>() / ttft.len() as f64);
    }
    result
}

#[derive(Debug, Serialize)]
pub struct MetricChange {
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub delta: Option<f64>,
    pub percent: Option<f64>,
}
impl MetricChange {
    fn new(before: Option<f64>, after: Option<f64>) -> Self {
        let delta = before.zip(after).map(|(a, b)| b - a);
        let percent = before
            .zip(delta)
            .filter(|(a, _)| *a != 0.0)
            .map(|(a, d)| d / a * 100.0);
        Self {
            before,
            after,
            delta,
            percent,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct MetricComparison {
    pub cost_usd: MetricChange,
    pub wall_time_ms: MetricChange,
    pub event_duration_ms: MetricChange,
    pub tokens: MetricChange,
    pub failures: MetricChange,
}
pub fn compare_metrics(left: &Run, right: &Run) -> MetricComparison {
    let a = metrics(left);
    let b = metrics(right);
    MetricComparison {
        cost_usd: MetricChange::new(
            a.cost_usd.filter(|_| a.priced_model_calls == a.model_calls),
            b.cost_usd.filter(|_| b.priced_model_calls == b.model_calls),
        ),
        wall_time_ms: MetricChange::new(a.wall_time_ms, b.wall_time_ms),
        event_duration_ms: MetricChange::new(Some(a.event_duration_ms), Some(b.event_duration_ms)),
        tokens: MetricChange::new(
            (a.measured_model_calls == a.model_calls).then_some(a.total_tokens as f64),
            (b.measured_model_calls == b.model_calls).then_some(b.total_tokens as f64),
        ),
        failures: MetricChange::new(Some(a.failures as f64), Some(b.failures as f64)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn usage_survives_redaction_and_unknown_price_stays_unknown() {
        let mut run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        assert_eq!(metrics(&run).cost_usd, None);
        run.events[1].attributes = json!({"input_tokens":42,"output_tokens":8,"access_token":"secret","cost_usd":0.04,"ttft_ms":20});
        run.redact();
        assert_eq!(run.events[1].attributes["access_token"], "[REDACTED]");
        let m = metrics(&run);
        assert_eq!(m.input_tokens, 42);
        assert_eq!(m.cost_usd, Some(0.04));
        assert_eq!(m.mean_ttft_ms, Some(20.0));
    }
}
