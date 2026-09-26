mod execute;
use anyhow::{Result, ensure};
pub use execute::{ExecutionResult, Executor, RerunOptions, rerun};
use refract_core::{ReplayPolicy, Run, id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Serialize, Deserialize)]
pub struct RecordedStep {
    pub event_id: String,
    pub output: Value,
}
/// Playback only: never invokes application code, providers, or tool callbacks.
pub fn exact(run: &Run) -> Result<Vec<RecordedStep>> {
    run.validate()?;
    run.events
        .iter()
        .map(|e| {
            ensure!(
                e.replay_policy != ReplayPolicy::Blocked,
                "event {} is blocked from replay",
                e.id
            );
            Ok(RecordedStep {
                event_id: e.id.clone(),
                output: e.output.clone(),
            })
        })
        .collect()
}
/// A fork is an unfinished prefix immediately before the selected event.
pub fn fork(run: &Run, from: &str) -> Result<Run> {
    run.validate()?;
    let index = run
        .events
        .iter()
        .position(|e| e.id == from)
        .ok_or_else(|| anyhow::anyhow!("fork event not found"))?;
    let mut branch = run.clone();
    branch.id = id("run");
    branch.events.truncate(index);
    branch.status = refract_core::Status::Running;
    branch.ended_at = None;
    for event in &mut branch.events {
        event.run_id = branch.id.clone();
    }
    branch.metadata["lineage"] =
        json!({"original_run_id":run.id,"fork_event":from,"branch_id":branch.id});
    branch.validate()?;
    Ok(branch)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playback_and_fork_do_not_execute_tools() {
        let mut run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        assert_eq!(exact(&run).unwrap().len(), 2);
        let branch = fork(&run, "evt_2").unwrap();
        assert_eq!(branch.events.len(), 1);
        assert_ne!(branch.id, run.id);
        run.events[0].replay_policy = ReplayPolicy::Blocked;
        assert!(exact(&run).is_err());
    }
}
