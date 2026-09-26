//! Validated ingestion with baseline secrets and configurable text/key redaction.
use anyhow::{Result, ensure};
use refract_core::Run;
use regex::Regex;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct RedactionPolicy {
    keys: Vec<String>,
    patterns: Vec<Regex>,
}
impl RedactionPolicy {
    pub fn new(keys: Vec<String>, patterns: Vec<String>) -> Result<Self> {
        ensure!(
            keys.len() <= 100 && patterns.len() <= 32,
            "redaction policy is too large"
        );
        let keys = keys
            .into_iter()
            .map(|k| k.to_lowercase().replace('-', "_"))
            .collect();
        let patterns = patterns
            .into_iter()
            .map(|p| {
                ensure!(p.len() <= 4096, "redaction expression is too long");
                Ok(Regex::new(&p)?)
            })
            .collect::<Result<_>>()?;
        Ok(Self { keys, patterns })
    }
    pub fn from_env() -> Result<Self> {
        let keys = std::env::var("REFRACT_REDACT_KEYS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .collect();
        let mut patterns: Vec<String> =
            serde_json::from_str(&std::env::var("REFRACT_REDACT_PATTERNS").unwrap_or("[]".into()))?;
        if std::env::var("REFRACT_REDACT_EMAILS").as_deref() == Ok("1") {
            patterns.push(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b".into());
        }
        Self::new(keys, patterns)
    }
    fn text(&self, text: &mut String) {
        for pattern in &self.patterns {
            *text = pattern.replace_all(text, "[REDACTED]").into_owned();
        }
    }
    fn value(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let key = key.to_lowercase().replace('-', "_");
                    if self.keys.iter().any(|s| key.contains(s)) {
                        *value = "[REDACTED]".into();
                    } else {
                        self.value(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(|value| self.value(value)),
            Value::String(text) => self.text(text),
            _ => (),
        }
    }
    pub fn normalize(&self, mut run: Run) -> Result<Run> {
        run.validate()?;
        run.redact();
        self.text(&mut run.name);
        self.value(&mut run.metadata);
        for event in &mut run.events {
            self.text(&mut event.name);
            self.value(&mut event.input);
            self.value(&mut event.output);
            self.value(&mut event.attributes);
        }
        Ok(run)
    }
}
pub fn normalize(run: Run) -> Result<Run> {
    RedactionPolicy::default().normalize(run)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_redacts_nested_keys_text_and_labels_before_indexing() {
        let mut run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        run.name = "case 123-45-6789".into();
        run.metadata = serde_json::json!({"customer_number":"private", "text":["call 123-45-6789"], "input_tokens":42});
        let run = RedactionPolicy::new(
            vec!["customer_number".into()],
            vec![r"\b\d{3}-\d{2}-\d{4}\b".into()],
        )
        .unwrap()
        .normalize(run)
        .unwrap();
        assert_eq!(run.name, "case [REDACTED]");
        assert_eq!(run.metadata["customer_number"], "[REDACTED]");
        assert_eq!(run.metadata["text"][0], "call [REDACTED]");
        assert_eq!(run.metadata["input_tokens"], 42);
        assert!(RedactionPolicy::new(vec![], vec!["[".into()]).is_err());
    }
}
