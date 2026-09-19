use anyhow::{Result, ensure};
use refract_core::{Run, SPEC_VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
const MAX_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Manifest {
    spec_version: String,
    files: BTreeMap<String, String>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Stable ZIP ordering, stored entries, fixed timestamps, checksums over exact bytes.
pub fn pack(run: &Run) -> Result<Vec<u8>> {
    run.validate()?;
    let mut run = run.clone();
    run.redact();
    let events = std::mem::take(&mut run.events);
    let execution = serde_json::to_vec(&run)?;
    let mut lines = Vec::new();
    for event in events {
        serde_json::to_writer(&mut lines, &event)?;
        lines.push(b'\n');
    }
    ensure!(
        (execution.len() + lines.len()) as u64 <= MAX_BYTES,
        "artifact exceeds size limit"
    );
    let manifest = Manifest {
        spec_version: SPEC_VERSION.into(),
        files: BTreeMap::from([
            ("execution.json".into(), hash(&execution)),
            ("events.jsonl".into(), hash(&lines)),
        ]),
    };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o600);
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest)?),
        ("execution.json", execution),
        ("events.jsonl", lines),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(&bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}
pub fn unpack(bytes: &[u8]) -> Result<Run> {
    ensure!(
        bytes.len() as u64 <= MAX_BYTES + 65536,
        "archive exceeds size limit"
    );
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    ensure!(zip.len() == 3, "v1 requires exactly three entries");
    let mut entries = BTreeMap::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        let name = file.name().to_owned();
        ensure!(
            ["manifest.json", "execution.json", "events.jsonl"].contains(&name.as_str()),
            "unexpected archive path"
        );
        ensure!(file.size() <= MAX_BYTES, "entry exceeds size limit");
        let mut body = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut body)?;
        total += body.len() as u64;
        ensure!(
            total <= MAX_BYTES + 4096,
            "expanded archive exceeds size limit"
        );
        ensure!(
            entries.insert(name, body).is_none(),
            "duplicate archive path"
        );
    }
    let manifest: Manifest = serde_json::from_slice(&entries["manifest.json"])?;
    ensure!(
        manifest.spec_version == SPEC_VERSION && manifest.files.len() == 2,
        "unsupported manifest"
    );
    for name in ["execution.json", "events.jsonl"] {
        ensure!(
            manifest.files.get(name) == Some(&hash(&entries[name])),
            "checksum mismatch for {name}"
        );
    }
    let mut run: Run = serde_json::from_slice(&entries["execution.json"])?;
    ensure!(run.events.is_empty(), "events must be in events.jsonl");
    for line in entries["events.jsonl"]
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
    {
        run.events.push(serde_json::from_slice(line)?);
    }
    run.validate()?;
    Ok(run)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_python_fixture() {
        let run = unpack(include_bytes!(
            "../../../tests/fixtures/simple-run/python.rfr"
        ))
        .unwrap();
        assert_eq!(run.id, "demo-1");
        assert_eq!(run.events.len(), 2);
    }

    #[test]
    fn rejects_duplicate_and_traversal_entries() {
        for names in [
            ["manifest.json", "manifest.json", "events.jsonl"],
            ["manifest.json", "../execution.json", "events.jsonl"],
        ] {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            for (index, name) in names.into_iter().enumerate() {
                // The zip writer rejects duplicate names itself; build distinct
                // entries and patch equal-length names to test the reader.
                let actual = if name == "manifest.json" && index != 0 {
                    "manifesz.json"
                } else {
                    name
                };
                zip.start_file(actual, SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"{}").unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let text = bytes.windows(13).position(|w| w == b"manifesz.json");
            let mut bytes = bytes;
            if text.is_some() {
                for i in 0..bytes.len().saturating_sub(12) {
                    if &bytes[i..i + 13] == b"manifesz.json" {
                        bytes[i + 7] = b't';
                    }
                }
            }
            assert!(unpack(&bytes).is_err());
        }
    }
    #[test]
    fn roundtrip_is_deterministic_and_tamper_evident() {
        let run: Run = serde_json::from_str(include_str!(
            "../../../tests/fixtures/simple-run/execution.json"
        ))
        .unwrap();
        let a = pack(&run).unwrap();
        assert_eq!(a, pack(&run).unwrap());
        assert_eq!(unpack(&a).unwrap(), run);
        let mut bad = a.clone();
        let pos = bad.windows(6).position(|w| w == b"demo-1").unwrap();
        bad[pos] = b'X';
        assert!(unpack(&bad).is_err());
    }
}
