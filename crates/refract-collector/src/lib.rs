//! Native batch ingestion boundary; OTLP conversion will use this validation path.
use anyhow::Result;
use refract_core::Run;
pub fn normalize(mut run: Run) -> Result<Run> {
    run.validate()?;
    run.redact();
    Ok(run)
}
