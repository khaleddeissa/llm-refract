//! Explicit, application-owned continuation. Artifacts never specify executable code.
use anyhow::{Result, ensure};
use refract_core::{Event, EventType, ReplayPolicy, Run, Status};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RerunOptions {
    pub from_event: String,
    pub model: Option<String>,
    pub replace_models: BTreeMap<String, String>,
    pub approved_events: BTreeSet<String>,
    pub allow_live: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResult {
    pub output: Value,
    #[serde(default)]
    pub attributes: Option<Value>,
}
pub trait Executor {
    /// Receive the step and the completed branch prefix, including newly produced outputs.
    fn execute(&self, event: &Event, context: &Run) -> Result<ExecutionResult>;
}
/// Async executors keep network I/O off the caller's runtime threads.
pub trait AsyncExecutor: Sync {
    fn execute(
        &self,
        event: &Event,
        context: &Run,
    ) -> impl std::future::Future<Output = Result<ExecutionResult>> + Send;
}
/// Authorize all events and validate binding references before any callback executes.
pub fn preflight(run: &Run, options: &RerunOptions) -> Result<Run> {
    run.validate()?;
    ensure!(
        options.allow_live,
        "executable rerun requires explicit live execution opt-in"
    );
    let mut branch = super::fork(run, &options.from_event)?;
    let start = branch.events.len();
    let mut known: BTreeSet<&str> = run.events[..start].iter().map(|e| e.id.as_str()).collect();
    branch.started_at = chrono::Utc::now();
    // Preflight the entire suffix before any side effect can run.
    for event in &run.events[start..] {
        ensure!(
            event.replay_policy != ReplayPolicy::Blocked,
            "event {} is blocked",
            event.id
        );
        let approval = event.replay_policy == ReplayPolicy::RequiresApproval
            || (matches!(
                event.kind,
                EventType::ToolCall
                    | EventType::Human
                    | EventType::Handoff
                    | EventType::StateChange
            ) && event.replay_policy != ReplayPolicy::ReadOnly);
        ensure!(
            !approval || options.approved_events.contains(&event.id),
            "event {} needs explicit approval",
            event.id
        );
        if let Some(bindings) = event.attributes.get("input_bindings") {
            let bindings = bindings
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("input_bindings must be an object"))?;
            ensure!(event.input.is_object(), "bound event input must be object");
            for binding in bindings.values() {
                let source = binding["event_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("binding event_id required"))?;
                ensure!(
                    known.contains(source),
                    "bindings must reference a preceding event"
                );
                if let Some(path) = binding.get("path") {
                    ensure!(
                        path.as_str()
                            .is_some_and(|p| p.is_empty() || p.starts_with('/')),
                        "binding path must be a JSON pointer"
                    );
                }
            }
        }
        known.insert(&event.id);
    }
    branch.metadata["rerun"] = json!({"from_event":options.from_event,"model":options.model,"replace_models":options.replace_models});
    Ok(branch)
}

fn prepare(old: &Event, branch: &Run, options: &RerunOptions) -> Result<Event> {
    let mut event = old.clone();
    event.run_id = branch.id.clone();
    if event.kind == EventType::Generation {
        let replacement = options.model.as_ref().or_else(|| {
            old.attributes["model"]
                .as_str()
                .and_then(|m| options.replace_models.get(m))
        });
        if let Some(model) = replacement {
            event.attributes["model"] = json!(model);
            if let Some(input) = event.input.as_object_mut() {
                input.insert("model".into(), json!(model));
            }
        }
    }
    if let Some(bindings) = event
        .attributes
        .get("input_bindings")
        .and_then(Value::as_object)
    {
        for (target, binding) in bindings {
            let source = binding["event_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("binding event_id required"))?;
            let pointer = binding.get("path").and_then(Value::as_str).unwrap_or("");
            let value = branch
                .events
                .iter()
                .find(|e| e.id == source)
                .and_then(|e| e.output.pointer(pointer))
                .ok_or_else(|| anyhow::anyhow!("binding source not available: {source}{pointer}"))?
                .clone();
            event
                .input
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("bound event input must be object"))?
                .insert(target.clone(), value);
        }
    }
    // Usage and latency belong to this execution, not to the baseline.
    for key in [
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "total_tokens",
        "cost_usd",
        "cost_estimated",
        "cost_is_estimate",
        "cache_hit",
        "stream_completed",
        "stream_incomplete",
        "capture_incomplete",
        "output_truncated",
        "ttft_ms",
    ] {
        event.attributes.as_object_mut().unwrap().remove(key);
    }
    Ok(event)
}

fn finish(event: &mut Event, result: ExecutionResult) -> Result<()> {
    event.output = result.output;
    event.status = Status::Completed;
    if let Some(attributes) = result.attributes {
        ensure!(
            attributes.is_object(),
            "executor attributes must be an object"
        );
        event
            .attributes
            .as_object_mut()
            .unwrap()
            .extend(attributes.as_object().unwrap().clone());
    }
    Ok(())
}

fn complete(mut branch: Run) -> Result<Run> {
    branch.status = Status::Completed;
    branch.ended_at = Some(chrono::Utc::now());
    branch.redact();
    branch.validate()?;
    Ok(branch)
}

pub fn rerun(run: &Run, options: &RerunOptions, executor: &dyn Executor) -> Result<Run> {
    let mut branch = preflight(run, options)?;
    for old in &run.events[branch.events.len()..] {
        let mut event = prepare(old, &branch, options)?;
        event.timestamp = chrono::Utc::now();
        let started = std::time::Instant::now();
        let result = executor.execute(&event, &branch)?;
        event.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        finish(&mut event, result)?;
        branch.events.push(event);
    }
    complete(branch)
}
pub async fn rerun_async(
    run: &Run,
    options: &RerunOptions,
    executor: &impl AsyncExecutor,
) -> Result<Run> {
    let mut branch = preflight(run, options)?;
    for old in &run.events[branch.events.len()..] {
        let mut event = prepare(old, &branch, options)?;
        event.timestamp = chrono::Utc::now();
        let started = std::time::Instant::now();
        let result = executor.execute(&event, &branch).await?;
        event.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        finish(&mut event, result)?;
        branch.events.push(event);
    }
    complete(branch)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Demo;
    impl Executor for Demo {
        fn execute(&self, event: &Event, context: &Run) -> Result<ExecutionResult> {
            Ok(ExecutionResult {
                output: json!({"model":event.attributes["model"],"prefix":context.events.len()}),
                attributes: None,
            })
        }
    }
    #[test]
    fn executable_branch_preserves_prefix_and_preflights() {
        let run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        let options = RerunOptions {
            from_event: "evt_2".into(),
            model: Some("candidate".into()),
            allow_live: true,
            ..Default::default()
        };
        let branch = rerun(&run, &options, &Demo).unwrap();
        assert_eq!(branch.events[0].output, run.events[0].output);
        assert_eq!(branch.events[1].output["model"], "candidate");
        assert_ne!(branch.id, run.id);
        let mut blocked = run.clone();
        blocked.events[1].replay_policy = ReplayPolicy::Blocked;
        assert!(rerun(&blocked, &options, &Demo).is_err());
        blocked.events[1].replay_policy = ReplayPolicy::RequiresApproval;
        assert!(rerun(&blocked, &options, &Demo).is_err());
    }
}
