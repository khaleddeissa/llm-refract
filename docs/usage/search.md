# Search and similar executions

The server indexes run names/status/timing/cost and event names/models/tool durations. All searches
are scoped by the authenticated API key. Search works with SQLite locally and PostgreSQL in shared
deployments. Structured and lexical searches stay local; semantic text search calls an explicitly configured embedding provider.

```bash
curl --fail --get "$REFRACT_URL/v1/search" \
  -H "Authorization: Bearer $REFRACT_API_KEY" \
  --data-urlencode 'q=customer' --data-urlencode 'status=failed' \
  --data-urlencode 'limit=20' --data-urlencode 'offset=0'
curl --fail --get "$REFRACT_URL/v1/search" \
  -H "Authorization: Bearer $REFRACT_API_KEY" \
  --data-urlencode 'tool=retrieve' --data-urlencode 'min_event_duration_ms=500'
```

| Parameter               | Behavior                                                                              |
| ----------------------- | ------------------------------------------------------------------------------------- |
| `q`                     | Case-insensitive substring of run or event names; SQL wildcard characters are literal |
| `status`                | `running`, `completed` or `failed`                                                    |
| `model`                 | Exact recorded `attributes.model` value                                               |
| `tool`                  | Exact name of a `tool.call` event                                                     |
| `min_duration_ms`       | Minimum recorded run duration                                                         |
| `min_event_duration_ms` | Minimum event duration; when combined with `tool`, both apply to the same event       |
| `min_cost_usd`          | Minimum sum of recorded, nonnegative `cost_usd` observations                          |
| `after`, `before`       | RFC3339 run start times, inclusive lower and exclusive upper bounds                   |
| `limit`, `offset`       | 1..1000 items (default 100); nonnegative offset (default 0)                           |

The response is `{"runs":[...],"total":N,"limit":20,"offset":0}`, ordered by start time descending
then ID descending. Filters combine with AND. It searches indexed labels, not arbitrary prompt/output
text. A missing cost observation contributes no stored cost, so use the metrics completeness fields
before interpreting cost-filter results. Pagination is offset-based and can shift under concurrent writes.

`GET /v1/runs/{id}/similar?limit=10` tokenizes the selected run's name and event names/inputs/outputs,
then scores lexical Jaccard overlap against the latest 1000 runs in the same scope. The response reports
`method: "lexical-jaccard-v1"`, `candidate_limit`, `total_candidates` and scored `runs`. Limits are 1..100;
nonzero offsets are unsupported. This gives an explainable local similarity heuristic, not semantic
embedding retrieval across the full database. The [Inspector](inspector.md) exposes search and comparison;
the [REST API](../api.md) is available to custom clients and [MCP tools](mcp.md).

## Automatic embeddings and model selection

Operators configure embedding profiles; project administrators enable profiles and select a default.
Users choose among enabled models in the Inspector's **Search mode** menu. No embedding weights are
bundled into Refract. A profile can point to a local model or a hosted service, and generation is
performed by the Rust service. The provider credential stays on the server.

Set `REFRACT_EMBEDDING_PROFILES_FILE` to a JSON file (or set `REFRACT_EMBEDDING_PROFILES` directly):

```json
[
  {
    "id": "openai-large",
    "label": "OpenAI large",
    "protocol": "openai",
    "endpoint": "https://api.openai.com/v1/embeddings",
    "model": "text-embedding-3-large",
    "dimensions": 3072,
    "credential_env": "OPENAI_API_KEY"
  },
  {
    "id": "local",
    "label": "Local embedding model",
    "protocol": "ollama",
    "endpoint": "http://127.0.0.1:11434/api/embed",
    "model": "embeddinggemma",
    "dimensions": 768
  }
]
```

The Ollama example is for local mode. Production profiles require HTTPS, including private endpoints.
Use `credential_env` to name an environment variable or its corresponding `_FILE` secret. Set
`auth_header` to `api-key` for Azure key authentication or `x-goog-api-key` for Gemini; the default
is `authorization` with a Bearer prefix. OAuth gateways can maintain rotating token files.
Redirects are rejected. Profiles optionally contain `scopes`, an array of
`{"organization":"acme","project":"support","environment":"production"}`; omitted/empty scopes
make a profile available to all authenticated projects on that server.

| Protocol | Endpoint contract                                                                   | Vector response                    |
| -------- | ----------------------------------------------------------------------------------- | ---------------------------------- |
| `openai` | OpenAI embeddings, Azure/Foundry deployments, LiteLLM, vLLM and compatible gateways | `data[0].embedding`                |
| `voyage` | Voyage text embeddings with document/query input types                              | `data[0].embedding`                |
| `ollama` | Ollama `/api/embed`                                                                 | `embeddings[0]`                    |
| `cohere` | Cohere v2 embed with float output and search input types                            | `embeddings.float[0]`              |
| `gemini` | Gemini `:embedContent` with retrieval task types                                    | `embedding.values`                 |
| `vertex` | Vertex text-embedding `:predict` with retrieval task types                          | `predictions[0].embeddings.values` |
| `custom` | Operator-owned JSON template and JSON pointer                                       | Configurable                       |

`endpoint` is the **complete request URL**, including deployment paths and required API-version query
parameters. It is never accepted from a search request. `dimensions` validates the returned vector;
set `request_dimensions: true` only when the selected model accepts a dimension parameter. Select
models and dimensions supported by your provider. Protocol formats follow the official
[OpenAI](https://developers.openai.com/api/docs/guides/embeddings),
[Voyage](https://docs.voyageai.com/reference/embeddings-api),
[Ollama](https://docs.ollama.com/api/embed), and
[Gemini](https://ai.google.dev/gemini-api/docs/embeddings) APIs.

A custom profile supplies `request_template` (an object) and `response_pointer` (a JSON pointer such
as `/vector`). Entire string values `$text`, `$model`, and `$task` are replaced as JSON values,
never interpolated into raw JSON. `$task` is `query` or `document`. This also supports custom
inference servers without another SDK dependency.

Enable models using an administrator key, or use **Project embeddings** in the Inspector:

```bash
curl --fail -X PUT "$REFRACT_URL/v1/admin/project/embeddings" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  -d '[{"profile":"local","is_default":true,"auto_index":true}]'
curl --fail "$REFRACT_URL/v1/search/text" \
  -H "Authorization: Bearer $REFRACT_API_KEY" -H 'Content-Type: application/json' \
  -d '{"query":"customer requested a refund","profile":"local","limit":10}'
```

New runs enqueue indexing in their ingestion transaction. Enabling a model queues existing runs.
`GET /v1/project/embeddings` reports pending, completed, and failed work. Workers use 120-second fenced leases with a 100-second job deadline; a crashed worker's job becomes claimable
again. Failed requests retry with exponential backoff up to ten attempts. Administrators can retry
and refresh the index with `POST /v1/admin/embeddings/reindex`. A repair sweep runs every minute.
Disabling automatic indexing cancels pending work; already stored vectors remain searchable until
run retention deletes them.

The versioned `run-text-v1` representation indexes the first **8,000 Unicode characters** of the run
name and recorded event names/inputs/outputs, after ingestion redaction. It excludes arbitrary
attributes and headers. Text is divided into at most four 2,000-character chunks; normalized chunk
vectors are averaged to represent the run. This bounds provider cost and worker time; semantic
search describes this representation, not every byte of a large artifact. Query text is limited to
8,000 UTF-8 bytes and is sent once. Provider failure is reported, never replaced by a fake vector.
For another representation, generate vectors in your application and use the vector API below.

Each profile has a namespace fingerprint covering its endpoint, model, dimension settings and text
representation. Changing these requires re-saving project settings and reindexing; changing labels
or rotating credentials does not. Model revisions behind an unchanged provider alias cannot be
detected automatically: change the configured model/profile when intentionally upgrading it.

**CLI**

```bash
export REFRACT_SERVER_URL=http://localhost:8000
# Set REFRACT_API_KEY or REFRACT_API_KEY_FILE for authenticated services.
refract embedding-models
refract search 'customer requested a refund' --profile local --limit 10
refract search 'customer requested a refund' --profile local --mode exact
```

**Python**

```python
from refract import RefractClient

client = RefractClient("http://localhost:8000")  # api_key=... for shared services
print(client.embedding_models())
results = client.search_text("customer requested a refund", profile="local")
for run in results["runs"]:
    print(run["id"], run["name"])
```

**TypeScript / Node**

```typescript
import { RefractClient } from "@llm-refract/sdk";

const client = new RefractClient(
  "http://localhost:8000",
  process.env.REFRACT_API_KEY,
);
console.log(await client.embeddingModels());
const results = await client.searchText("customer requested a refund", {
  profile: "local",
});
console.log(results.matches);
```

MCP exposes `embedding_models()` and `search_text(query, profile?, limit?)`. Text search is read-only
with respect to recorded runs but can call a configured external provider; its MCP annotation
identifies that external interaction. A reader key is sufficient. Configuration and reindexing
require an administrator.

## Bring your own vectors

Applications can generate embeddings with any library/provider and use `PUT /v1/runs/{id}/embedding`
with `{"model":"my-embedder-v2","values":[0.4,0.9,0.1]}`. Search with
`POST /v1/search/vector` and `{"embedding":{"model":"my-embedder-v2","values":[0.4,0.8,0.2]},"limit":10}`.
These numbers illustrate the shape; they are not real model outputs. Here `model` is a namespace
identifier and does not trigger a provider request. `RefractClient.search_vector` (Python),
`searchVector` (TypeScript), and MCP `vector_search` expose the same operation.

Vectors have 1–4096 finite dimensions and nonzero magnitude. Query and stored vectors must come from
the same embedding model and representation. Namespaces are isolated by organization, project,
environment, model identifier and dimension. Embeddings use configured payload encryption and
expire with their recording. Cosine scores range from -1 to 1.

## Exact and approximate indexing

`mode` accepts `auto` (default), `exact`, or `approximate` on both search endpoints. Exact search
streams the full matching namespace from a consistent SQL statement and retains only the best
1–100 results. It has no 10,000-candidate cutoff. For larger namespaces, automatic mode builds an
[HNSW graph](https://docs.rs/instant-distance/latest/instant_distance/struct.HnswMap.html) in a blocking worker and
reuses it for subsequent queries. Approximate mode can use the graph for small namespaces too.
HNSW trades exhaustive ranking for lower query cost; request exact mode when completeness matters.

The server keeps an LRU cache of graphs, with an estimated default budget of 256 MiB. Set
`REFRACT_VECTOR_CACHE_MB` (16–32768) to match your deployment. Namespaces below 256 vectors use
exact retrieval in automatic mode. If a namespace exceeds the graph budget, search falls back to
streaming exact retrieval over **all** candidates, never an arbitrary prefix. Memory estimates
include graph edges, IDs and construction copies; account for in-flight queries and other service
memory when setting container limits. A single graph builds at a time; cached searches continue.

Canonical vectors persist in SQLite/PostgreSQL with configured payload encryption. Graphs exist
only in memory and rebuild after restart; no plaintext vector/index files are written. Transactional
namespace generation counters invalidate caches after writes or retention, including writes by
another replica. A concurrent change during construction/search causes an exact fallback. Updating
an existing vector replaces it and invalidates the graph; encryption-key rotation preserves the
vector values and does not require re-embedding.

Large active indexes require appropriate memory and CPU provisioning. The first query after a
restart or namespace update may rebuild a graph. Storage tests cover 10,050 vectors, approximate
recall, exact retrieval, tenant isolation, replacement through an independent connection pool,
reopen/rebuild, and retention. The Inspector uses automatic mode; SDK/API callers can select exact
mode for comparisons and audits.
