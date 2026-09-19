import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type {
  Execution,
  ExecutionEvent,
} from "../../packages/typescript/src/index.js";
import { request } from "./api";
import "./style.css";
function App() {
  const [runs, setRuns] = useState<Execution[]>([]);
  const [run, setRun] = useState<Execution>();
  const [selected, setSelected] = useState<ExecutionEvent>();
  const [error, setError] = useState("");
  const [result, setResult] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [compareId, setCompareId] = useState("");
  const choose = (r: Execution) => {
    setRun(r);
    setSelected(r.events[0]);
    setResult(undefined);
    setCompareId("");
  };
  async function refresh() {
    setError("");
    setLoading(true);
    try {
      const loaded = await request<Execution[]>("/v1/runs");
      setRuns(loaded);
      if (!run && loaded[0]) choose(loaded[0]);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void refresh();
  }, []);
  async function action(kind: "replay" | "fork" | "diff") {
    if (!run) return;
    setBusy(true);
    setError("");
    try {
      if (kind === "fork") {
        if (!selected) return;
        const branch = await request<Execution>(
          `/v1/runs/${encodeURIComponent(run.id)}/fork`,
          { from_event: selected.id },
        );
        setRuns([branch, ...runs]);
        choose(branch);
      } else if (kind === "diff")
        setResult(
          await request("/v1/diff", { left: run.id, right: compareId }),
        );
      else
        setResult(
          await request(`/v1/runs/${encodeURIComponent(run.id)}/replay`, {
            mode: "exact",
          }),
        );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="app">
      <aside>
        <a className="brand" href="/">
          ◈ <span>refract</span>
        </a>
        <p className="eyebrow">EXECUTION WORKSPACE</p>
        <div className="nav-title">
          Runs{" "}
          <button onClick={() => void refresh()} aria-label="Refresh runs">
            ↻
          </button>
        </div>
        <nav aria-label="Recorded runs">
          {runs.map((r) => (
            <button
              disabled={busy}
              className={`run-link ${r.id === run?.id ? "active" : ""}`}
              key={r.id}
              onClick={() => choose(r)}
            >
              <span className={`dot ${r.status}`} />
              <span>
                {r.name}
                <small>{r.id.slice(0, 20)}</small>
              </span>
            </button>
          ))}
        </nav>
        <footer>
          <span className="dot completed" /> Local workspace
          <small>refract.execution.v1</small>
        </footer>
      </aside>
      <main>
        <header>
          <span>
            Workspace <span className="slash">/</span> Executions
          </span>
          <span className="version">FOUNDATION · 0.1</span>
        </header>
        {error && (
          <div role="alert" className="error">
            {error}
          </div>
        )}
        {!run ? (
          <section className="empty">
            <div className="symbol">◈</div>
            <p className="eyebrow">EVERY EXECUTION TELLS A STORY</p>
            <h1>
              {loading ? "Loading executions…" : "Start with a recorded run."}
            </h1>
            <p>
              Inspect what happened. Replay recorded outputs. Compare what
              changed.
            </p>
            <pre>
              curl -X POST localhost:8000/v1/runs \\{"\n"} -H 'Content-Type:
              application/json' \\{"\n"} --data-binary
              @tests/fixtures/simple-run/execution.json
            </pre>
          </section>
        ) : (
          <>
            <section className="heading">
              <div>
                <p className="eyebrow">EXECUTION DETAIL</p>
                <h1>{run.name}</h1>
                <p className="muted mono">{run.id}</p>
              </div>
              <span className={`badge ${run.status}`}>{run.status}</span>
            </section>
            <section className="metrics">
              <div>
                <label>EVENTS</label>
                <strong>{run.events.length.toString().padStart(2, "0")}</strong>
              </div>
              <div>
                <label>EVENT DURATION · SUM</label>
                <strong>
                  {run.events
                    .reduce((n, e) => n + (e.duration_ms ?? 0), 0)
                    .toLocaleString()}{" "}
                  <small>ms</small>
                </strong>
              </div>
              <div>
                <label>STARTED</label>
                <strong className="date">
                  {new Date(run.started_at).toLocaleString()}
                </strong>
              </div>
              <div>
                <label>REPLAY MODE</label>
                <strong className="date">Recorded outputs</strong>
              </div>
            </section>
            <section className="toolbar">
              <div>
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => void action("replay")}
                >
                  ↻ Replay recorded
                </button>
                <button
                  disabled={busy || !selected}
                  onClick={() => void action("fork")}
                >
                  ⑂ Fork before event
                </button>
                <a
                  className="button"
                  href={`/v1/runs/${encodeURIComponent(run.id)}/artifact`}
                >
                  ↓ Export .rfr
                </a>
              </div>
              <div>
                <select
                  aria-label="Compare with run"
                  value={compareId}
                  onChange={(e) => setCompareId(e.target.value)}
                >
                  <option value="">Compare with…</option>
                  {runs
                    .filter((r) => r.id !== run.id)
                    .map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.name} · {r.id.slice(-8)}
                      </option>
                    ))}
                </select>
                <button
                  disabled={busy || !compareId}
                  onClick={() => void action("diff")}
                >
                  Diff
                </button>
              </div>
            </section>
            <section className="execution">
              <div className="timeline">
                <div className="panel-heading">
                  <h2>Execution timeline</h2>
                  <span>{run.events.length} steps</span>
                </div>
                {run.events.length === 0 && (
                  <p className="muted">This fork has no recorded events yet.</p>
                )}
                {run.events.map((e, i) => (
                  <button
                    disabled={busy}
                    className={`event ${selected?.id === e.id ? "selected" : ""}`}
                    key={e.id}
                    onClick={() => setSelected(e)}
                  >
                    <span className="step">
                      {String(i + 1).padStart(2, "0")}
                    </span>
                    <span className="event-title">
                      {e.name}
                      <small>
                        {e.type} · {e.status}
                      </small>
                      <span
                        className="bar"
                        style={{
                          width: `${Math.max(4, ((e.duration_ms ?? 0) / Math.max(1, ...run.events.map((v) => v.duration_ms ?? 0))) * 100)}%`,
                        }}
                      />
                    </span>
                    <span className="duration">{e.duration_ms} ms</span>
                  </button>
                ))}
              </div>
              <div className="details">
                <div className="panel-heading">
                  <h2>Event inspector</h2>
                  <span className="mono">{selected?.type ?? "—"}</span>
                </div>
                {selected ? (
                  <>
                    <h3>{selected.name}</h3>
                    <p className="muted mono">{selected.id}</p>
                    {(
                      [
                        ["Input", selected.input],
                        ["Output", selected.output],
                        ["Attributes", selected.attributes],
                        ["Parent event", selected.parent_id],
                        ["Replay policy", selected.replay_policy],
                      ] as const
                    ).map(([title, value]) => (
                      <div key={title}>
                        <label>{title}</label>
                        <pre>{JSON.stringify(value, null, 2)}</pre>
                      </div>
                    ))}
                  </>
                ) : (
                  <p>Select an event to inspect its recorded data.</p>
                )}
              </div>
            </section>
            {result !== undefined && (
              <section className="result" aria-live="polite">
                <div className="panel-heading">
                  <h2>Execution result</h2>
                  <button onClick={() => setResult(undefined)}>Close</button>
                </div>
                <pre>{JSON.stringify(result, null, 2)}</pre>
              </section>
            )}
            <details className="metadata">
              <summary>Run metadata</summary>
              <pre>{JSON.stringify(run.metadata, null, 2)}</pre>
            </details>
            <p className="footnote">
              Recorded playback returns captured outputs. Forks preserve the
              prefix before the selected event; they do not execute new steps.
            </p>
          </>
        )}
      </main>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
