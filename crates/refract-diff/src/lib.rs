mod semantic;
use refract_core::Run;
pub use semantic::{
    Grade, Grader, OfflineGrader, SemanticOptions, SemanticReport, compare_semantic,
    compare_with_grader,
};
use serde::Serialize;
use serde_json::{Value, json};
#[derive(Debug, Serialize)]
pub struct Difference {
    pub index: usize,
    pub left: Option<Value>,
    pub right: Option<Value>,
}
fn semantic(event: &refract_core::Event, run: &Run) -> Value {
    json!({"type":event.kind,"name":event.name,"status":event.status,"input":event.input,
        "output":event.output,"attributes":event.attributes,"replay_policy":event.replay_policy,
        "parent_index":event.parent_id.as_ref().and_then(|p| run.events.iter().position(|e| &e.id==p))})
}
/// Ordered semantic diff; generated IDs and timing are intentionally ignored.
pub fn compare(left: &Run, right: &Run) -> Vec<Difference> {
    (0..left.events.len().max(right.events.len()))
        .filter_map(|index| {
            let l = left.events.get(index).map(|e| semantic(e, left));
            let r = right.events.get(index).map(|e| semantic(e, right));
            (l != r).then_some(Difference {
                index,
                left: l,
                right: r,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_first_divergence_and_added_events() {
        let a: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        let mut b = a.clone();
        b.id = "other".into();
        assert!(compare(&a, &b).is_empty());
        b.events[1].output = json!("changed");
        assert_eq!(compare(&a, &b)[0].index, 1);
        b.events.clear();
        assert_eq!(compare(&a, &b).len(), 2);
    }
}
