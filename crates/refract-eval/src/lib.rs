//! Reproducible evaluation datasets: explicit baselines and freshly recorded candidates.
use anyhow::{Result, ensure};
use refract_core::{Metrics, Run, metrics};
use refract_diff::{Grader, OfflineGrader, SemanticOptions, SemanticReport, compare_with_grader};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub baseline: String,
    pub candidate: String,
    #[serde(default)]
    pub options: Option<SemanticOptions>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub version: u32,
    #[serde(default)]
    pub options: SemanticOptions,
    pub cases: Vec<Case>,
}
#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
    pub report: Option<SemanticReport>,
    pub baseline: Option<Metrics>,
    pub candidate: Option<Metrics>,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub passed: bool,
    pub total: usize,
    pub regressions: usize,
    pub equivalent: usize,
    pub improved: usize,
    pub cases: Vec<CaseResult>,
}
fn read(root: &Path, name: &str) -> Result<Run> {
    let relative = Path::new(name);
    ensure!(
        relative
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "dataset paths must remain within dataset directory"
    );
    let path = root.join(relative).canonicalize()?;
    ensure!(
        path.starts_with(root.canonicalize()?),
        "dataset symlink escapes directory"
    );
    ensure!(
        std::fs::metadata(&path)?.len() <= 17 * 1024 * 1024,
        "artifact exceeds size limit"
    );
    refract_artifact::unpack(&std::fs::read(path)?)
}
pub fn evaluate(root: &Path, dataset: &Dataset) -> Result<Evaluation> {
    evaluate_with_grader(root, dataset, &OfflineGrader)
}
pub fn evaluate_with_grader(
    root: &Path,
    dataset: &Dataset,
    grader: &dyn Grader,
) -> Result<Evaluation> {
    ensure!(dataset.version == 1, "unsupported dataset version");
    ensure!(!dataset.cases.is_empty(), "dataset has no cases");
    dataset.options.validate().map_err(anyhow::Error::msg)?;
    let mut names = BTreeSet::new();
    let mut result = Evaluation {
        passed: true,
        total: dataset.cases.len(),
        regressions: 0,
        equivalent: 0,
        improved: 0,
        cases: vec![],
    };
    for case in &dataset.cases {
        ensure!(
            !case.name.trim().is_empty() && names.insert(&case.name),
            "empty or duplicate case name"
        );
        let pair =
            read(root, &case.baseline).and_then(|a| read(root, &case.candidate).map(|b| (a, b)));
        match pair {
            Ok((a, b)) => {
                let report = compare_with_grader(
                    &a,
                    &b,
                    case.options.as_ref().unwrap_or(&dataset.options),
                    grader,
                );
                let before = metrics(&a);
                let after = metrics(&b);
                if !report.passed {
                    result.regressions += 1;
                } else if before
                    .cost_usd
                    .zip(after.cost_usd)
                    .is_some_and(|(a, b)| b < a)
                    || before
                        .wall_time_ms
                        .zip(after.wall_time_ms)
                        .is_some_and(|(a, b)| b < a)
                {
                    result.improved += 1;
                } else {
                    result.equivalent += 1;
                }
                result.cases.push(CaseResult {
                    name: case.name.clone(),
                    passed: report.passed,
                    error: None,
                    report: Some(report),
                    baseline: Some(before),
                    candidate: Some(after),
                });
            }
            Err(error) => {
                result.regressions += 1;
                result.cases.push(CaseResult {
                    name: case.name.clone(),
                    passed: false,
                    error: Some(error.to_string()),
                    report: None,
                    baseline: None,
                    candidate: None,
                });
            }
        }
    }
    result.passed = result.regressions == 0;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_candidate_is_failed_case_not_skipped() {
        let data = Dataset {
            version: 1,
            options: Default::default(),
            cases: vec![Case {
                name: "missing".into(),
                baseline: "no.rfr".into(),
                candidate: "missing.rfr".into(),
                options: None,
            }],
        };
        let report = evaluate(Path::new("."), &data).unwrap();
        assert!(!report.passed);
        assert_eq!(report.regressions, 1);
    }
    #[test]
    fn rejects_empty_dataset() {
        assert!(
            evaluate(
                Path::new("."),
                &Dataset {
                    version: 1,
                    options: Default::default(),
                    cases: vec![]
                }
            )
            .is_err()
        );
    }
}
