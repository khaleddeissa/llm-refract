use refract_core::Run;
fn main() -> anyhow::Result<()> {
    let run: Run = serde_json::from_str(include_str!(
        "../../../tests/fixtures/simple-run/execution.json"
    ))?;
    let bytes = refract_artifact::pack(&run)?;
    let restored = refract_artifact::unpack(&bytes)?;
    assert_eq!(run, restored);
    println!(
        "{}: {} events, {} readable bytes",
        restored.name,
        restored.events.len(),
        bytes.len()
    );
    Ok(())
}
