import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type {
  Execution,
  ExecutionEvent,
} from "../../../packages/typescript/src/index.js";
import { request, setApiKey, download } from "./api";
import { ExecutionGraph } from "./graph";
import { metrics, number, difference } from "./metrics";
import "./style.css";
interface SemanticReport {
  passed: boolean;
  equivalent: number;
  changed: number;
  differences: {
    index: number;
    category: string;
    reason: string;
    grade?: { score: number; grader: string; reason: string } | null;
  }[];
  budget_violations: string[];
}
function App() {
  const [runs, setRuns] = useState<Execution[]>([]);
  const [run, setRun] = useState<Execution>();
  const [selected, setSelected] = useState<ExecutionEvent>();
  const [error, setError] = useState("");
  const [result, setResult] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [compareId, setCompareId] = useState("");
  const [semantic, setSemantic] = useState(true);
  const semanticResult =
    result && typeof result === "object" && "semantic_report" in result
      ? (result.semantic_report as SemanticReport | undefined)
      : undefined;
  const [credential, setCredential] = useState("");
  const [authenticated, setAuthenticated] = useState(false);
  const [filters, setFilters] = useState({
    q: "",
    status: "",
    model: "",
    tool: "",
    min_duration_ms: "",
  });
  const [offset, setOffset] = useState(0);
  const [total, setTotal] = useState(0);
  const summary = run ? metrics(run) : undefined;
  const compare = runs.find((item) => item.id === compareId);
  const candidate = compare ? metrics(compare) : undefined;
  const choose = (r: Execution) => {
    setRun(r);
    setSelected(r.events[0]);
    setResult(undefined);
    setCompareId("");
  };
  async function refresh(page = 0) {
    setError("");
    setLoading(true);
    try {
      const query = new URLSearchParams({ limit: "100", offset: String(page) });
      for (const [key, value] of Object.entries(filters))
        if (value.trim()) query.set(key, value.trim());
      const loaded = await request<{ runs: Execution[]; total: number }>(
        `/v1/search?${query}`,
      );
      setRuns(loaded.runs);
      setOffset(page);
      setTotal(loaded.total);
      if (
        loaded.runs.length &&
        !loaded.runs.some((item) => item.id === run?.id)
      )
        choose(loaded.runs[0]);
      if (!loaded.runs.length) {
        setRun(undefined);
        setSelected(undefined);
      }
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
          await request("/v1/diff", {
            left: run.id,
            right: compareId,
            semantic,
          }),
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
        <form
          className="search-form"
          onSubmit={(event) => {
            event.preventDefault();
            void refresh();
          }}
        >
          <label htmlFor="run-query">Search executions</label>
          <input
            id="run-query"
            placeholder="Text or run name"
            value={filters.q}
            onChange={(event) =>
              setFilters({ ...filters, q: event.target.value })
            }
          />
          <details>
            <summary>Filter executions</summary>
            <label>
              Status
              <select
                aria-label="Filter status"
                value={filters.status}
                onChange={(event) =>
                  setFilters({ ...filters, status: event.target.value })
                }
              >
                <option value="">Any status</option>
                <option>completed</option>
                <option>failed</option>
                <option>running</option>
              </select>
            </label>
            {(
              [
                ["model", "Model"],
                ["tool", "Tool name"],
                ["min_duration_ms", "Minimum latency (ms)"],
              ] as const
            ).map(([key, label]) => (
              <label key={key}>
                {label}
                <input
                  value={filters[key]}
                  type={key === "min_duration_ms" ? "number" : "text"}
                  min="0"
                  onChange={(event) =>
                    setFilters({ ...filters, [key]: event.target.value })
                  }
                />
              </label>
            ))}
          </details>
          <button type="submit" disabled={loading}>
            Search
          </button>
        </form>
        <p className="muted result-count">{total} matching runs</p>
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
        <div className="pagination">
          <button
            disabled={offset === 0 || loading}
            onClick={() => void refresh(Math.max(0, offset - 100))}
          >
            Previous
          </button>
          <button
            disabled={offset + 100 >= total || loading}
            onClick={() => void refresh(offset + 100)}
          >
            Next
          </button>
        </div>
        <form
          className="credentials"
          onSubmit={(event) => {
            event.preventDefault();
            setApiKey(credential.trim());
            setAuthenticated(Boolean(credential.trim()));
            setCredential("");
            void refresh();
          }}
        >
          <label htmlFor="api-key">API key (tab memory only)</label>
          <input
            id="api-key"
            type="password"
            autoComplete="off"
            value={credential}
            onChange={(event) => setCredential(event.target.value)}
          />
          <button type="submit">
            {authenticated ? "Replace / clear key" : "Use API key"}
          </button>
          {authenticated && <small>Key active until page reload.</small>}
        </form>
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
          <span className="version">EXECUTION INSPECTOR</span>
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
            <section className="metrics" aria-label="Execution metrics">
              <div>
                <label>COST · RECORDED ESTIMATE</label>
                <strong>
                  {summary?.cost === undefined
                    ? "—"
                    : `$${summary.cost.toFixed(6)}`}
                </strong>
                <small>
                  {summary?.costCoverage}/{summary?.generations} model calls
                  priced
                </small>
              </div>
              <div>
                <label>WALL LATENCY</label>
                <strong>
                  {number(summary?.latency)} <small>ms</small>
                </strong>
              </div>
              <div>
                <label>TOKENS · RECORDED</label>
                <strong>{number(summary?.tokens)}</strong>
                <small>
                  {number(summary?.input)} in / {number(summary?.output)} out ·{" "}
                  {summary?.usageCoverage}/{summary?.generations} calls
                </small>
              </div>
              <div>
                <label>CALLS</label>
                <strong>
                  {summary?.generations}{" "}
                  <small>model / {summary?.tools} tool</small>
                </strong>
              </div>
              <div>
                <label>TTFT · MEAN</label>
                <strong>
                  {number(summary?.ttft, 1)} <small>ms</small>
                </strong>
              </div>
              <div>
                <label>CACHED INPUT TOKENS</label>
                <strong>{number(summary?.cache)}</strong>
              </div>
            </section>
            <div className="highlights">
              <span>
                Most expensive:{" "}
                <button
                  disabled={!summary?.expensive}
                  onClick={() => setSelected(summary?.expensive)}
                >
                  {summary?.expensive?.name ?? "Not recorded"}
                </button>
              </span>
              <span>
                Slowest event:{" "}
                <button
                  disabled={!summary?.slowest}
                  onClick={() => setSelected(summary?.slowest)}
                >
                  {summary?.slowest?.name ?? "No events"}
                </button>
              </span>
            </div>
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
                <button
                  disabled={busy}
                  onClick={() => {
                    void download(
                      `/v1/runs/${encodeURIComponent(run.id)}/artifact`,
                      `${run.id}.rfr`,
                    ).catch((error) => setError(String(error)));
                  }}
                >
                  ↓ Export .rfr
                </button>
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
                <label className="semantic-toggle">
                  <input
                    type="checkbox"
                    checked={semantic}
                    onChange={(event) => setSemantic(event.target.checked)}
                  />
                  Semantic comparison
                </label>
                <button
                  disabled={busy || !compareId}
                  onClick={() => void action("diff")}
                >
                  Diff
                </button>
              </div>
            </section>
            {summary && candidate && (
              <section className="comparison" aria-label="Metric comparison">
                <div className="panel-heading">
                  <h2>Metric comparison</h2>
                  <span>
                    {run.name} → {compare?.name}
                  </span>
                </div>
                <table>
                  <thead>
                    <tr>
                      <th>Metric</th>
                      <th>Current run</th>
                      <th>Compared run</th>
                      <th>Change</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(
                      [
                        ["Cost (USD)", "cost", 6],
                        ["Latency (ms)", "latency", 1],
                        ["Tokens", "tokens", 0],
                        ["TTFT (ms)", "ttft", 1],
                        ["Cached tokens", "cache", 0],
                      ] as const
                    ).map(([label, key, digits]) => (
                      <tr key={key}>
                        <th>{label}</th>
                        <td>{number(summary[key], digits)}</td>
                        <td>{number(candidate[key], digits)}</td>
                        <td>{difference(summary[key], candidate[key])}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            )}
            <ExecutionGraph
              events={run.events}
              selected={selected?.id}
              onSelect={setSelected}
            />
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
                {semanticResult && (
                  <section aria-label="Semantic comparison results">
                    <h3>
                      {semanticResult.passed
                        ? "Comparison passed"
                        : "Review required"}
                    </h3>
                    <p>
                      {semanticResult.equivalent} equivalent events ·{" "}
                      {semanticResult.changed} changed events
                    </p>
                    <p className="muted">
                      The offline grader compares normalized words, numbers and
                      negation. Review meaning and factual accuracy when they
                      matter.
                    </p>
                    {semanticResult.differences.length > 0 && (
                      <table>
                        <thead>
                          <tr>
                            <th>Event</th>
                            <th>Category</th>
                            <th>Explanation</th>
                          </tr>
                        </thead>
                        <tbody>
                          {semanticResult.differences.map(
                            (difference, index) => (
                              <tr key={index}>
                                <td>{difference.index + 1}</td>
                                <td>{difference.category}</td>
                                <td>
                                  {difference.reason}
                                  {difference.grade
                                    ? ` (${difference.grade.grader}, ${(difference.grade.score * 100).toFixed(1)}%)`
                                    : ""}
                                </td>
                              </tr>
                            ),
                          )}
                        </tbody>
                      </table>
                    )}
                    {semanticResult.budget_violations.map(
                      (violation, index) => (
                        <p key={index} className="error">
                          {violation}
                        </p>
                      ),
                    )}
                  </section>
                )}
                <details>
                  <summary>Full execution result</summary>
                  <pre>{JSON.stringify(result, null, 2)}</pre>
                </details>
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
