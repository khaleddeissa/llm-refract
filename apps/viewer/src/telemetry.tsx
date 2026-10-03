import React, { useEffect, useState } from "react";
import { request } from "./api";
type Record = {
  kind: string;
  trace_id: string;
  payload: {
    severity_text?: string;
    body?: unknown;
    metric?: { name?: string };
    [key: string]: unknown;
  };
};
export function TelemetryPanel({ authRevision }: { authRevision: number }) {
  const [kind, setKind] = useState("logs");
  const [trace, setTrace] = useState("");
  const [records, setRecords] = useState<Record[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [next, setNext] = useState(0);
  useEffect(() => {
    setRecords([]);
    setError("");
    setNext(0);
  }, [authRevision]);
  async function load(offset = 0) {
    setBusy(true);
    setError("");
    try {
      const query = new URLSearchParams({
        kind,
        limit: "50",
        offset: String(offset),
        ...(kind === "logs" && trace ? { trace_id: trace } : {}),
      });
      const page = await request<{ records: Record[]; next_offset: number }>(
        `/v1/telemetry?${query}`,
      );
      setRecords(offset ? [...records, ...page.records] : page.records);
      setNext(page.next_offset);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="telemetry-panel">
      <summary>OpenTelemetry logs and metrics</summary>
      <div className="toolbar">
        <label>
          Signal{" "}
          <select
            disabled={busy}
            aria-label="Telemetry signal"
            value={kind}
            onChange={(e) => {
              setKind(e.target.value);
              setRecords([]);
              setNext(0);
            }}
          >
            <option value="logs">Logs</option>
            <option value="metrics">Metrics</option>
          </select>
        </label>
        {kind === "logs" && (
          <label>
            Trace ID{" "}
            <input
              disabled={busy}
              aria-label="Telemetry trace ID"
              value={trace}
              onChange={(e) => {
                setTrace(e.target.value);
                setRecords([]);
                setNext(0);
              }}
              placeholder="Optional 32-character trace ID"
            />
          </label>
        )}
        <button disabled={busy} onClick={() => void load()}>
          Load telemetry
        </button>
      </div>
      {error && <p role="alert">{error}</p>}
      <p>{records.length} telemetry records loaded</p>
      {records.map((record, index) => (
        <details key={`${kind}:${index}`}>
          <summary>
            {record.payload.metric?.name ??
              record.payload.severity_text ??
              "Log"}
            {record.trace_id ? ` · ${record.trace_id}` : ""}
          </summary>
          <pre>{JSON.stringify(record.payload, null, 2)}</pre>
        </details>
      ))}
      {records.length > 0 && (
        <button disabled={busy} onClick={() => void load(next)}>
          Load more telemetry
        </button>
      )}
    </details>
  );
}
