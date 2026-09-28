import React, { useEffect, useState } from "react";
import { request } from "./api";

interface Model {
  id: string;
  label: string;
  model: string;
  dimensions: number;
}
interface Selection {
  profile: string;
  is_default: boolean;
  auto_index: boolean;
}
interface Settings {
  profiles: Selection[];
  jobs: { pending: number; done: number; failed: number };
}

export function EmbeddingControls({
  profile,
  onChange,
  refreshToken,
}: {
  profile: string;
  onChange: (profile: string) => void;
  refreshToken: number;
}) {
  const [models, setModels] = useState<Model[]>([]);
  const [settings, setSettings] = useState<Settings>();
  const [draft, setDraft] = useState<Selection[]>([]);
  const [admin, setAdmin] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  async function load() {
    const [available, current, identity] = await Promise.all([
      request<{ models: Model[] }>("/v1/embedding-models"),
      request<Settings>("/v1/project/embeddings"),
      request<{ role: string }>("/v1/auth/me"),
    ]);
    setModels(available.models);
    setSettings(current);
    setDraft(
      current.profiles.map(({ profile, is_default, auto_index }) => ({
        profile,
        is_default,
        auto_index,
      })),
    );
    setAdmin(identity.role === "admin");
  }
  useEffect(() => {
    let active = true;
    // This component is also used before authentication; the main workspace reports auth errors.
    void (async () => {
      try {
        const [available, current, identity] = await Promise.all([
          request<{ models: Model[] }>("/v1/embedding-models"),
          request<Settings>("/v1/project/embeddings"),
          request<{ role: string }>("/v1/auth/me"),
        ]);
        if (active) {
          setModels(available.models);
          setSettings(current);
          setDraft(
            current.profiles.map(({ profile, is_default, auto_index }) => ({
              profile,
              is_default,
              auto_index,
            })),
          );
          setAdmin(identity.role === "admin");
          setError("");
        }
      } catch {
        if (active) {
          setModels([]);
          setSettings(undefined);
          setAdmin(false);
        }
      }
    })();
    return () => {
      active = false;
    };
  }, [refreshToken]);
  async function save(reindex = false) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      if (reindex) await request("/v1/admin/embeddings/reindex", {});
      else await request("/v1/admin/project/embeddings", draft, "PUT");
      await load();
      if (!draft.some((item) => item.profile === profile)) onChange("");
      setNotice(
        reindex
          ? "Reindex queued."
          : "Embedding settings saved. Indexing continues in the background.",
      );
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  }
  if (!models.length) return null;
  return (
    <section
      className="embedding-controls"
      aria-label="Semantic search settings"
    >
      <label>
        Search mode
        <select
          aria-label="Search mode"
          value={profile}
          onChange={(e) => onChange(e.target.value)}
        >
          <option value="">Text and filters</option>
          {settings?.profiles.map((p) => (
            <option key={p.profile} value={p.profile}>
              Semantic ·{" "}
              {models.find((m) => m.id === p.profile)?.label ?? p.profile}
            </option>
          ))}
        </select>
      </label>
      {profile && (
        <small>
          Searches by meaning using the selected project model. Results are
          ranked by cosine similarity.
        </small>
      )}
      <details>
        <summary>Project embeddings</summary>
        {settings && (
          <p className="muted">
            {settings.jobs.done} indexed · {settings.jobs.pending} pending ·{" "}
            {settings.jobs.failed} failed
          </p>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={() => void load().catch((e) => setError(String(e)))}
        >
          Refresh index status
        </button>
        {admin && (
          <>
            <p>
              Select models available to this project. Automatic indexing sends
              recorded, redacted text to these models.
            </p>
            {models.map((model) => {
              const selected = draft.find((p) => p.profile === model.id);
              return (
                <fieldset key={model.id}>
                  <label>
                    <input
                      type="checkbox"
                      checked={Boolean(selected)}
                      onChange={(e) => {
                        let next = draft.filter((p) => p.profile !== model.id);
                        if (e.target.checked)
                          next = [
                            ...next,
                            {
                              profile: model.id,
                              is_default: next.length === 0,
                              auto_index: true,
                            },
                          ];
                        if (next.length && !next.some((p) => p.is_default))
                          next[0] = { ...next[0], is_default: true };
                        setDraft(next);
                      }}
                    />
                    {model.label}
                  </label>
                  <small>
                    {model.model} · {model.dimensions} dimensions
                  </small>
                  {selected && (
                    <>
                      <label>
                        <input
                          type="radio"
                          name="default-embedding"
                          checked={selected.is_default}
                          onChange={() =>
                            setDraft(
                              draft.map((p) => ({
                                ...p,
                                is_default: p.profile === model.id,
                              })),
                            )
                          }
                        />
                        Default search model
                      </label>
                      <label>
                        <input
                          type="checkbox"
                          checked={selected.auto_index}
                          onChange={(e) =>
                            setDraft(
                              draft.map((p) =>
                                p.profile === model.id
                                  ? { ...p, auto_index: e.target.checked }
                                  : p,
                              ),
                            )
                          }
                        />
                        Automatically index runs
                      </label>
                    </>
                  )}
                </fieldset>
              );
            })}
            <button type="button" disabled={busy} onClick={() => void save()}>
              Save embedding settings
            </button>
            <button
              type="button"
              disabled={busy || !settings?.profiles.length}
              onClick={() => void save(true)}
            >
              Reindex recorded runs
            </button>
          </>
        )}
        {notice && <p role="status">{notice}</p>}
        {error && <p role="alert">{error}</p>}
      </details>
    </section>
  );
}
