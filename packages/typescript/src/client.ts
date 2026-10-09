import { showCommunityMessage } from "./community.js";
import type { Execution } from "./index.js";

export interface GenerationModel {
  id: string;
  label: string;
  model: string;
  protocol: string;
  grading: boolean;
  max_output_tokens: number;
}
export interface EmbeddingModel {
  id: string;
  label: string;
  model: string;
  protocol: string;
  dimensions: number;
  namespace: string;
}
export interface EmbeddingSelection {
  profile: string;
  is_default: boolean;
  auto_index: boolean;
}
export interface EmbeddingSettings {
  profiles: (EmbeddingSelection & { model: string })[];
  jobs: { pending: number; done: number; failed: number };
}
export interface VectorMatch {
  run_id: string;
  score: number;
}
export interface TextSearchResult {
  runs: Execution[];
  matches: VectorMatch[];
  total: number;
  profile: string;
  namespace: string;
}
/** Service requests use an explicitly configured endpoint; redirects never receive credentials. */
export class RefractClient {
  private readonly url: string;
  constructor(
    url = "http://127.0.0.1:8000",
    private readonly apiKey?: string,
  ) {
    const parsed = new URL(url);
    if (
      !["http:", "https:"].includes(parsed.protocol) ||
      parsed.username ||
      parsed.password ||
      parsed.search ||
      parsed.hash
    )
      throw new Error(
        "Service URL requires HTTP(S) without credentials, query, or fragment",
      );
    this.url = url.replace(/\/$/, "");
    showCommunityMessage();
  }
  async request<T>(
    path: string,
    body?: unknown,
    method?: string,
    timeoutMs = 35_000,
  ): Promise<T> {
    if (!path.startsWith("/v1/") && !path.startsWith("/scim/v2/"))
      throw new Error("Request path must start with /v1/ or /scim/v2/");
    const response = await fetch(this.url + path, {
      method: method ?? (body === undefined ? "GET" : "POST"),
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      headers: {
        "Content-Type": "application/json",
        ...(this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {}),
      },
      redirect: "error",
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!response.ok)
      throw new Error(`Refract request failed (${response.status})`);
    const reader = response.body?.getReader();
    if (!reader) throw new Error("Empty service response");
    const chunks: Uint8Array[] = [];
    let size = 0;
    try {
      while (true) {
        const chunk = await reader.read();
        if (chunk.done) break;
        size += chunk.value.byteLength;
        if (size > 17 * 1024 * 1024)
          throw new Error("Service response exceeds 17 MiB");
        chunks.push(chunk.value);
      }
    } finally {
      await reader.cancel();
    }
    return JSON.parse(Buffer.concat(chunks).toString("utf8")) as T;
  }
  async embeddingModels(): Promise<EmbeddingModel[]> {
    return (
      await this.request<{ models: EmbeddingModel[] }>("/v1/embedding-models")
    ).models;
  }
  embeddingSettings(): Promise<EmbeddingSettings> {
    return this.request("/v1/project/embeddings");
  }
  configureEmbeddings(
    profiles: EmbeddingSelection[],
  ): Promise<{ profiles: EmbeddingSettings["profiles"] }> {
    return this.request("/v1/admin/project/embeddings", profiles, "PUT");
  }
  reindexEmbeddings(): Promise<{ queued: number }> {
    return this.request("/v1/admin/embeddings/reindex", {});
  }
  searchText(
    query: string,
    options: {
      profile?: string;
      limit?: number;
      mode?: "auto" | "exact" | "approximate";
    } = {},
  ): Promise<TextSearchResult> {
    return this.request("/v1/search/text", { query, ...options });
  }
  searchVector(
    model: string,
    values: number[],
    limit = 20,
    mode: "auto" | "exact" | "approximate" = "auto",
  ): Promise<VectorMatch[]> {
    return this.request("/v1/search/vector", {
      embedding: { model, values },
      limit,
      mode,
    });
  }
  async generationModels(): Promise<GenerationModel[]> {
    return (
      await this.request<{ models: GenerationModel[] }>("/v1/generation-models")
    ).models;
  }
  rerun(
    runId: string,
    options: {
      profile: string;
      from_event: string;
      allow_live?: boolean;
      reuse_recorded?: string[];
      approved_events?: string[];
    },
  ): Promise<Execution> {
    if (!runId || runId === "." || runId === "..")
      throw new Error("Invalid run id");
    return this.request(
      `/v1/runs/${encodeURIComponent(runId)}/rerun`,
      options,
      undefined,
      130_000,
    );
  }
  compare(
    left: string,
    right: string,
    options: {
      semantic?: boolean;
      grader?: string;
      allow_live?: boolean;
      options?: Record<string, number>;
    } = {},
  ): Promise<Record<string, unknown>> {
    return this.request(
      "/v1/diff",
      { left, right, semantic: true, ...options },
      undefined,
      130_000,
    );
  }
  telemetry(
    kind: "logs" | "metrics" = "logs",
    options: { trace_id?: string; limit?: number; offset?: number } = {},
  ): Promise<{
    records: {
      kind: string;
      trace_id: string;
      payload: Record<string, unknown>;
    }[];
    next_offset: number;
  }> {
    const query = new URLSearchParams({
      kind,
      ...Object.fromEntries(
        Object.entries(options).map(([key, value]) => [key, String(value)]),
      ),
    });
    return this.request(`/v1/telemetry?${query}`);
  }
}
