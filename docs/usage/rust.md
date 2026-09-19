# Rust embedding mode

The engine crates can be used directly without HTTP, Docker or an SDK. Within this workspace,
`refract-core` supplies `Run`, `Event`, enums and validation. `refract-artifact` supplies `pack` and
`unpack`; `refract-replay` supplies `exact` and `fork`; `refract-diff` supplies `compare`.
Crates are not published to crates.io yet; use path dependencies from a source checkout.

```rust
use refract_core::Run;

fn main() -> anyhow::Result<()> {
    let run: Run = serde_json::from_str(include_str!("../../../tests/fixtures/simple-run/execution.json"))?;
    run.validate()?;
    let recording = refract_artifact::pack(&run)?;
    let restored = refract_artifact::unpack(&recording)?;
    assert_eq!(run, restored);
    Ok(())
}
```

Run the maintained example with `cargo run -p refract-artifact --example inspect`.
It reads the shared fixture, packs readable bytes and validates a round trip. Rust tests additionally
exercise legacy ZIP compatibility, corruption/path rejection, replay policies, API routes and storage.
For embedding in an external project, replace `include_str!` with your own recorded JSON or construct
a `Run` with `Run::new` and your events.
