//! Explainable offline comparison with a replaceable output grader.
use refract_core::{Run, compare_metrics, metrics};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SemanticOptions {
    pub similarity_threshold: f64,
    pub max_cost_increase_percent: Option<f64>,
    pub max_latency_increase_percent: Option<f64>,
    pub max_token_increase_percent: Option<f64>,
}
impl Default for SemanticOptions {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.75,
            max_cost_increase_percent: None,
            max_latency_increase_percent: None,
            max_token_increase_percent: None,
        }
    }
}
impl SemanticOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !self.similarity_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.similarity_threshold)
        {
            return Err("similarity_threshold must be between zero and one".into());
        }
        for value in [
            self.max_cost_increase_percent,
            self.max_latency_increase_percent,
            self.max_token_increase_percent,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() || value < 0.0 {
                return Err("budget thresholds must be finite and nonnegative".into());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Grade {
    pub score: f64,
    pub equivalent: bool,
    pub reason: String,
    pub grader: String,
}
/// A grader can use a local model, hosted model, embeddings, or domain-specific assertions.
/// Implementations must return an error on unavailable/invalid grading, never a passing score.
pub trait Grader {
    fn grade(&self, left: &Value, right: &Value, threshold: f64) -> Result<Grade, String>;
}
pub struct OfflineGrader;
fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '.')
        .filter(|w| !w.is_empty())
        .map(|w| {
            match w {
                "one" => "1",
                "two" => "2",
                "three" => "3",
                "four" => "4",
                "five" => "5",
                "six" => "6",
                "seven" => "7",
                "eight" => "8",
                "nine" => "9",
                "ten" => "10",
                "working" => "business",
                "approximately" | "about" => "around",
                "arrive" | "expect" => "receive",
                _ => w,
            }
            .trim_matches('.')
            .to_owned()
        })
        .filter(|w| {
            ![
                "the", "a", "an", "your", "you", "will", "in", "within", "to", "is", "are", "be",
                "around",
            ]
            .contains(&w.as_str())
        })
        .collect()
}
fn compare_value(left: &Value, right: &Value) -> (f64, bool) {
    if left == right {
        return (1.0, true);
    }
    match (left, right) {
        (Value::String(a), Value::String(b)) => {
            let a = words(a);
            let b = words(b);
            let facts = |set: &BTreeSet<String>| {
                set.iter()
                    .filter(|s| {
                        s.chars().any(|c| c.is_ascii_digit())
                            || ["not", "no", "never", "cannot", "without"].contains(&s.as_str())
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>()
            };
            if facts(&a) != facts(&b) {
                return (0.0, false);
            }
            let size = a.union(&b).count();
            if size == 0 {
                return (0.0, false);
            }
            (a.intersection(&b).count() as f64 / size as f64, true)
        }
        (Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => a
            .iter()
            .map(|(k, v)| compare_value(v, &b[k]))
            .fold((1.0, true), |(score, safe), (s, f)| {
                (score.min(s), safe && f)
            }),
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .map(|(a, b)| compare_value(a, b))
            .fold((1.0, true), |(score, safe), (s, f)| {
                (score.min(s), safe && f)
            }),
        _ => (0.0, false),
    }
}
impl Grader for OfflineGrader {
    fn grade(&self, left: &Value, right: &Value, threshold: f64) -> Result<Grade, String> {
        let (score, facts) = compare_value(left, right);
        Ok(Grade {
            score,
            equivalent: facts && score >= threshold,
            reason: if left == right {
                "identical output"
            } else if !facts {
                "number, negation, type or structure changed"
            } else {
                "normalized token overlap; human or model review may still be needed"
            }
            .into(),
            grader: "offline-token-overlap-v1".into(),
        })
    }
}
#[derive(Debug, Serialize)]
pub struct SemanticDifference {
    pub index: usize,
    pub category: String,
    pub grade: Option<Grade>,
    pub reason: String,
}
#[derive(Debug, Serialize)]
pub struct SemanticReport {
    pub passed: bool,
    pub equivalent: usize,
    pub changed: usize,
    pub differences: Vec<SemanticDifference>,
    pub budget_violations: Vec<String>,
    pub metric_changes: refract_core::metrics::MetricComparison,
}
fn shape(event: &refract_core::Event, run: &Run) -> Value {
    let mut attrs = event.attributes.clone();
    if let Some(map) = attrs.as_object_mut() {
        for name in [
            "total_tokens",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "cost_usd",
            "ttft_ms",
            "model",
            "provider",
        ] {
            map.remove(name);
        }
    }
    let mut input = event.input.clone();
    if event.kind == refract_core::EventType::Generation
        && let Some(map) = input.as_object_mut()
    {
        map.remove("model");
    }
    json!({"type":event.kind,"name":event.name,"status":event.status,"input":input,"attributes":attrs,"replay_policy":event.replay_policy,"parent_index":event.parent_id.as_ref().and_then(|id|run.events.iter().position(|e|&e.id==id))})
}
pub fn compare_semantic(left: &Run, right: &Run, options: &SemanticOptions) -> SemanticReport {
    compare_with_grader(left, right, options, &OfflineGrader)
}
pub fn compare_with_grader(
    left: &Run,
    right: &Run,
    options: &SemanticOptions,
    grader: &dyn Grader,
) -> SemanticReport {
    let mut report = SemanticReport {
        passed: true,
        equivalent: 0,
        changed: 0,
        differences: vec![],
        budget_violations: vec![],
        metric_changes: compare_metrics(left, right),
    };
    if let Err(error) = options.validate() {
        report.passed = false;
        report.budget_violations.push(error);
        return report;
    }
    for index in 0..left.events.len().max(right.events.len()) {
        let (a, b) = match (left.events.get(index), right.events.get(index)) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                report.differences.push(SemanticDifference {
                    index,
                    category: "structure".into(),
                    grade: None,
                    reason: "event added or removed".into(),
                });
                report.changed += 1;
                continue;
            }
        };
        if shape(a, left) != shape(b, right) {
            report.differences.push(SemanticDifference {
                index,
                category: "behavior".into(),
                grade: None,
                reason: "input, tool, status, relationship, attribute or policy changed".into(),
            });
            report.changed += 1;
            continue;
        }
        match grader.grade(&a.output, &b.output, options.similarity_threshold) {
            Ok(grade) if grade.score.is_finite() && (0.0..=1.0).contains(&grade.score) => {
                let equivalent = grade.equivalent && grade.score >= options.similarity_threshold;
                if equivalent {
                    report.equivalent += 1;
                } else {
                    report.changed += 1;
                }
                if a.output != b.output || !equivalent {
                    report.differences.push(SemanticDifference {
                        index,
                        category: if equivalent { "wording" } else { "output" }.into(),
                        reason: grade.reason.clone(),
                        grade: Some(grade),
                    });
                }
            }
            result => {
                report.changed += 1;
                report.differences.push(SemanticDifference {
                    index,
                    category: "grader_error".into(),
                    grade: None,
                    reason: result.err().unwrap_or("invalid grader score".into()),
                });
            }
        }
    }
    let a = metrics(left);
    let b = metrics(right);
    for (name, threshold, old, new) in [
        (
            "cost",
            options.max_cost_increase_percent,
            a.cost_usd.filter(|_| a.priced_model_calls == a.model_calls),
            b.cost_usd.filter(|_| b.priced_model_calls == b.model_calls),
        ),
        (
            "latency",
            options.max_latency_increase_percent,
            a.wall_time_ms,
            b.wall_time_ms,
        ),
        (
            "tokens",
            options.max_token_increase_percent,
            (a.measured_model_calls == a.model_calls).then_some(a.total_tokens as f64),
            (b.measured_model_calls == b.model_calls).then_some(b.total_tokens as f64),
        ),
    ] {
        if let Some(limit) = threshold {
            match old.zip(new) {
                Some((old, new)) if new <= old * (1.0 + limit / 100.0) => (),
                Some(_) => report
                    .budget_violations
                    .push(format!("{name} increased beyond {limit}%")),
                None => report.budget_violations.push(format!(
                    "{name} budget cannot be checked: incomplete measurements"
                )),
            }
        }
    }
    if left.status != right.status {
        report.budget_violations.push("run status changed".into());
    }
    report.passed = report.changed == 0 && report.budget_violations.is_empty();
    report
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wording_and_factual_changes_are_different() {
        let grader = OfflineGrader;
        let a = json!("Your refund will arrive within 5 business days.");
        let b = json!("Expect the refund in approximately five working days.");
        assert!(grader.grade(&a, &b, 0.75).unwrap().equivalent);
        assert!(
            !grader
                .grade(
                    &json!("Returns within 30 days"),
                    &json!("Returns within 14 days"),
                    0.1
                )
                .unwrap()
                .equivalent
        );
        assert!(
            !grader
                .grade(
                    &json!("payment approved"),
                    &json!("payment not approved"),
                    0.1
                )
                .unwrap()
                .equivalent
        );
    }
    #[test]
    fn missing_cost_fails_budget_and_invalid_threshold_fails_closed() {
        let run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        let options = SemanticOptions {
            max_cost_increase_percent: Some(20.0),
            ..Default::default()
        };
        assert!(!compare_semantic(&run, &run, &options).passed);
        assert!(
            !compare_semantic(
                &run,
                &run,
                &SemanticOptions {
                    similarity_threshold: 2.0,
                    ..Default::default()
                }
            )
            .passed
        );
    }
}
