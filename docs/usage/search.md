# Search and similar executions

The server indexes run names/status/timing/cost and event names/models/tool durations. All searches
are scoped by the authenticated API key. Search works with SQLite locally and PostgreSQL in shared
deployments; it does not send prompts to an embedding provider.

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

## Vector search and embedding-model selection

**Your application chooses and runs the embedding model.** Refract does not bundle an embedder,
download weights or send recording content to an embedding provider. Use an open-source local model,
a hosted embedding API, or LiteLLM's embedding interface in your application. You choose which text to
embed: for example, a failure summary, selected event outputs, or a sanitized recording summary.

The flow is:

1. Generate a vector from selected recording text using your chosen model.
2. Store it with `PUT /v1/runs/{id}/embedding`, including a model/version identifier.
3. Embed the search query using the **same model, version and preprocessing**.
4. Send that query vector to `POST /v1/search/vector` with the same identifier.
5. Refract normalizes vectors, computes exact cosine scores and returns ranked `{run_id, score}` matches.

```http
PUT /v1/runs/run_123/embedding
Content-Type: application/json
Authorization: Bearer WRITER_KEY

{"model":"my-embedder-v2","values":[0.4,0.9,0.1]}
```

```http
POST /v1/search/vector
Content-Type: application/json
Authorization: Bearer READER_KEY

{"embedding":{"model":"my-embedder-v2","values":[0.4,0.8,0.2]},"limit":10}
```

These three-dimensional vectors are illustrative fixtures, not actual model outputs. The `model` field
is an identifier, **not a request for the server to load that model**. An identifier such as
`text-embedding-3-large` or a Voyage model name does not trigger an API call. A future automatic text
search interface would need project-level provider/model settings, credential references and an
embedding-generation adapter. An end-user model picker is optional; one configured model per project
can provide a simpler search experience. Multiple models may be stored for
the same run; writing the same run/model replaces that vector. Candidates must match the authenticated
organization/project/environment, model identifier and dimensions. Model names are application-owned;
Refract cannot detect that two incorrectly labeled vectors came from different models. Use a new
identifier when changing model revision, dimensions, text selection or preprocessing, then re-embed.

Vectors must contain 1–4096 finite values and have nonzero magnitude. Exact search supports at most
10,000 candidates per model/dimension namespace and returns 1–100 matches; larger candidate sets are
rejected rather than partially searched. Scores range from -1 to 1. This is bounded exact retrieval,
not an approximate index for millions of embeddings. Embeddings use configured payload encryption and
expire with their recording. MCP exposes the same operation through `vector_search(model, values, limit)`.

Structured `/v1/search` and lexical `/similar` continue to work without embeddings. The Inspector's
search form uses structured search; vector retrieval is currently available through REST and MCP.
