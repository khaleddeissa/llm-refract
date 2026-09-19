use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

pub const SPEC_VERSION: &str = "refract.execution.v1";
pub fn id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Completed,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Generation,
    #[serde(rename = "tool.call")]
    ToolCall,
    Retrieval,
    Decision,
    #[serde(rename = "state.change")]
    StateChange,
    Checkpoint,
    Handoff,
    Human,
    Artifact,
    Error,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplayPolicy {
    ReadOnly,
    Mock,
    #[default]
    Recorded,
    Live,
    RequiresApproval,
    Blocked,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub run_id: String,
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: EventType,
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: f64,
    pub status: Status,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default = "object")]
    pub attributes: Value,
    #[serde(default)]
    pub replay_policy: ReplayPolicy,
}
fn object() -> Value {
    serde_json::json!({})
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub spec_version: String,
    pub id: String,
    pub name: String,
    pub status: Status,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default = "object")]
    pub metadata: Value,
    #[serde(default)]
    pub events: Vec<Event>,
}
impl Run {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            spec_version: SPEC_VERSION.into(),
            id: id("run"),
            name: name.into(),
            status: Status::Running,
            started_at: Utc::now(),
            ended_at: None,
            metadata: object(),
            events: vec![],
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.spec_version == SPEC_VERSION,
            "unsupported execution version"
        );
        ensure!(
            !self.id.is_empty() && !self.name.trim().is_empty(),
            "run id/name cannot be empty"
        );
        ensure!(self.metadata.is_object(), "metadata must be an object");
        if let Some(end) = self.ended_at {
            ensure!(end >= self.started_at, "run ends before it starts");
        }
        ensure!(
            self.status == Status::Running || self.ended_at.is_some(),
            "finished run needs ended_at"
        );
        let mut seen = HashSet::new();
        for e in &self.events {
            ensure!(e.run_id == self.id, "event belongs to another run");
            ensure!(
                !e.id.is_empty() && !e.name.trim().is_empty(),
                "event id/name cannot be empty"
            );
            ensure!(
                e.duration_ms.is_finite() && e.duration_ms >= 0.0,
                "invalid duration"
            );
            ensure!(e.attributes.is_object(), "attributes must be an object");
            if let Some(p) = &e.parent_id {
                ensure!(seen.contains(p), "parent must precede child");
            }
            ensure!(seen.insert(e.id.clone()), "duplicate event id");
        }
        Ok(())
    }
    pub fn redact(&mut self) {
        redact(&mut self.metadata);
        for e in &mut self.events {
            redact(&mut e.input);
            redact(&mut e.output);
            redact(&mut e.attributes);
        }
    }
}
/// Baseline key-based redaction. Free-text PII requires an application policy.
pub fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = key.to_lowercase().replace('-', "_");
                if [
                    "password",
                    "secret",
                    "token",
                    "api_key",
                    "authorization",
                    "cookie",
                    "email",
                ]
                .iter()
                .any(|s| key.contains(s))
                {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    redact(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact),
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_nested_secrets() {
        let mut v = serde_json::json!({"nested": [{"api-key":"secret", "answer":42}]});
        redact(&mut v);
        assert_eq!(v["nested"][0]["api-key"], "[REDACTED]");
        assert_eq!(v["nested"][0]["answer"], 42);
    }
    #[test]
    fn validates_fixture_and_rejects_bad_parent() {
        let mut r: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        r.validate().unwrap();
        r.events[0].parent_id = Some("missing".into());
        assert!(r.validate().is_err());
    }
}
