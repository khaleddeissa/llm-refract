//! Explicit command plugins use bounded JSON over stdio; never a shell command from an artifact.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct JsonCommand {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub timeout: Duration,
}
impl JsonCommand {
    pub fn call(&self, value: &Value) -> Result<Value> {
        let mut child = Command::new(&self.executable)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("could not start explicit plugin command")?;
        let mut input = child.stdin.take().unwrap();
        let bytes = serde_json::to_vec(value)?;
        let writer = std::thread::spawn(move || input.write_all(&bytes));
        let output = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let result = output
                .take(1024 * 1024 + 1)
                .read_to_end(&mut data)
                .map(|_| data);
            let _ = tx.send(result);
        });
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("plugin command timed out")
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if !status.success() {
            bail!("plugin command failed: {status}")
        }
        let data = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .context("plugin stdout did not close")??;
        anyhow::ensure!(data.len() <= 1024 * 1024, "plugin output exceeds 1 MiB");
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("plugin input thread failed"))??;
        serde_json::from_slice(&data).context("plugin must return one JSON object")
    }
}
impl refract_diff::Grader for JsonCommand {
    fn grade(
        &self,
        left: &Value,
        right: &Value,
        threshold: f64,
    ) -> Result<refract_diff::Grade, String> {
        self.call(&serde_json::json!({"left":left,"right":right,"threshold":threshold}))
            .and_then(|value| Ok(serde_json::from_value(value)?))
            .map_err(|e| e.to_string())
    }
}
impl refract_replay::Executor for JsonCommand {
    fn execute(
        &self,
        event: &refract_core::Event,
        context: &refract_core::Run,
    ) -> Result<refract_replay::ExecutionResult> {
        Ok(serde_json::from_value(self.call(
            &serde_json::json!({"event":event,"context":context}),
        )?)?)
    }
}
