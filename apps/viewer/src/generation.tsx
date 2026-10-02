import React, { useState } from "react";
import type {
  Execution,
  ExecutionEvent,
} from "../../../packages/typescript/src/index.js";
import type { GenerationModel } from "../../../packages/typescript/src/client.js";
import { request } from "./api";

export function RerunControls({
  run,
  selected,
  models,
  onBranch,
}: {
  run: Execution;
  selected?: ExecutionEvent;
  models: GenerationModel[];
  onBranch: (branch: Execution) => void;
}) {
  const [profile, setProfile] = useState("");
  const [consent, setConsent] = useState(false);
  const [reuse, setReuse] = useState(false);
  const [approve, setApprove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!models.length) return null;
  const start = run.events.findIndex((e) => e.id === selected?.id);
  const suffix = start < 0 ? [] : run.events.slice(start);
  const reused = suffix.filter((e) => e.type !== "generation");
  const approvals = suffix.filter(
    (e) =>
      e.replay_policy === "REQUIRES_APPROVAL" ||
      (["tool.call", "human", "handoff", "state.change"].includes(e.type) &&
        e.replay_policy !== "READ_ONLY"),
  );
  const blocked = suffix.some((e) => e.replay_policy === "BLOCKED");
  async function rerun() {
    if (!selected) return;
    setBusy(true);
    setError("");
    try {
      const branch = await request<Execution>(
        `/v1/runs/${encodeURIComponent(run.id)}/rerun`,
        {
          profile,
          from_event: selected.id,
          allow_live: consent,
          reuse_recorded: reuse ? reused.map((e) => e.id) : [],
          approved_events: approve ? approvals.map((e) => e.id) : [],
        },
      );
      onBranch(branch);
      setConsent(false);
      setReuse(false);
      setApprove(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="rerun-panel">
      <summary>Rerun with a model</summary>
      <p>
        Start at <strong>{selected?.name ?? "a selected event"}</strong>. A new
        branch preserves the original run.
      </p>
      <label>
        Generation model{" "}
        <select
          aria-label="Generation model"
          value={profile}
          onChange={(e) => {
            setProfile(e.target.value);
            setConsent(false);
          }}
        >
          <option value="">Select a configured model…</option>
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label} · {m.model}
            </option>
          ))}
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={consent}
          onChange={(e) => setConsent(e.target.checked)}
        />
        Authorize {suffix.filter((e) => e.type === "generation").length}{" "}
        provider calls; usage may be charged.
      </label>
      {reused.length > 0 && (
        <label>
          <input
            type="checkbox"
            checked={reuse}
            onChange={(e) => setReuse(e.target.checked)}
          />
          Reuse recorded outputs without running these steps:{" "}
          {reused.map((e) => `${e.name} (${e.id})`).join(", ")}
        </label>
      )}
      {approvals.length > 0 && (
        <label>
          <input
            type="checkbox"
            checked={approve}
            onChange={(e) => setApprove(e.target.checked)}
          />
          Approve replay policies for:{" "}
          {approvals.map((e) => `${e.name} (${e.id})`).join(", ")}
        </label>
      )}
      {blocked && (
        <p role="alert">
          This suffix contains a blocked event and cannot be rerun.
        </p>
      )}
      <button
        disabled={
          busy ||
          !profile ||
          !models.some((m) => m.id === profile) ||
          !consent ||
          !selected ||
          blocked ||
          (reused.length > 0 && !reuse) ||
          (approvals.length > 0 && !approve)
        }
        onClick={() => void rerun()}
      >
        {busy ? "Running model steps…" : "Create model branch"}
      </button>
      {error && <p role="alert">{error}</p>}
    </details>
  );
}
