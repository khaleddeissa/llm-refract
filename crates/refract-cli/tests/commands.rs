use std::{fs, process::Command};
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_refract"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn pack_inspect_convert_and_detect_divergence() {
    let dir = std::env::temp_dir().join(refract_core::id("refract-cli"));
    fs::create_dir(&dir).unwrap();
    let source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/simple-run/execution.json"
    );
    let original = dir.join("original.rfr");
    let original = original.to_str().unwrap();
    assert!(cli(&["pack", source, "-o", original]).status.success());
    assert!(
        fs::read_to_string(original)
            .unwrap()
            .contains("refract.artifact.v1")
    );
    assert!(cli(&["validate", original]).status.success());
    let inspected: serde_json::Value =
        serde_json::from_slice(&cli(&["inspect", original]).stdout).unwrap();
    assert_eq!(inspected["id"], "demo-1");
    assert!(!cli(&["pack", source, "-o", original]).status.success());
    let branch = dir.join("branch.rfr");
    let branch = branch.to_str().unwrap();
    assert!(
        cli(&["fork", original, "--from", "evt_2", "-o", branch])
            .status
            .success()
    );
    let diff = cli(&["diff", original, branch]);
    assert_eq!(diff.status.code(), Some(1));
    let changes: serde_json::Value = serde_json::from_slice(&diff.stdout).unwrap();
    assert_eq!(changes[0]["index"], 1);
    let legacy = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/simple-run/python.rfr"
    );
    let converted = dir.join("converted.rfr");
    assert!(
        cli(&["pack", legacy, "-o", converted.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        cli(&["diff", original, converted.to_str().unwrap()])
            .status
            .success()
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn semantic_eval_and_explicit_executor() {
    let dir = std::env::temp_dir().join(refract_core::id("refract-experiment"));
    fs::create_dir(&dir).unwrap();
    let source = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/simple-run/execution.json"
    );
    let baseline = dir.join("baseline.rfr");
    assert!(
        cli(&["pack", source, "-o", baseline.to_str().unwrap()])
            .status
            .success()
    );
    let metrics = cli(&["metrics", baseline.to_str().unwrap()]);
    assert!(metrics.status.success());
    let value: serde_json::Value = serde_json::from_slice(&metrics.stdout).unwrap();
    assert_eq!(value["model_calls"], 1);
    assert!(value["cost_usd"].is_null());
    assert!(
        cli(&[
            "diff",
            baseline.to_str().unwrap(),
            baseline.to_str().unwrap(),
            "--semantic"
        ])
        .status
        .success()
    );
    assert_eq!(
        cli(&[
            "diff",
            baseline.to_str().unwrap(),
            baseline.to_str().unwrap(),
            "--semantic",
            "--max-cost-increase-percent",
            "10"
        ])
        .status
        .code(),
        Some(1)
    );
    fs::write(dir.join("dataset.json"),r#"{"version":1,"cases":[{"name":"identity","baseline":"baseline.rfr","candidate":"baseline.rfr"}]}"#).unwrap();
    assert!(cli(&["eval", dir.to_str().unwrap()]).status.success());
    // Explicit trusted executable receives one JSON request. No command lives in the recording.
    let script = dir.join("executor.py");
    fs::write(&script,"import json,sys\nr=json.load(sys.stdin)\nprint(json.dumps({'output': {'text':'candidate','model':r['event']['attributes']['model']}}))\n").unwrap();
    let output = dir.join("candidate.rfr");
    let base = [
        "rerun",
        baseline.to_str().unwrap(),
        "--from",
        "evt_2",
        "--executor",
        "python3",
        "--executor-arg",
        script.to_str().unwrap(),
        "--model",
        "demo-candidate",
        "-o",
        output.to_str().unwrap(),
    ];
    assert!(!cli(&base).status.success());
    let mut args = base.to_vec();
    args.push("--allow-live");
    assert!(cli(&args).status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&cli(&["inspect", output.to_str().unwrap()]).stdout).unwrap();
    assert_eq!(value["events"][1]["output"]["model"], "demo-candidate");
    assert_eq!(value["events"][0]["id"], "evt_1");
    fs::remove_dir_all(dir).unwrap();
}
