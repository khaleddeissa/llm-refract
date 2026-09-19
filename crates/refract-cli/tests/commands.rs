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
