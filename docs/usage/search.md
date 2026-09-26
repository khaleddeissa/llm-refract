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

| Parameter | Behavior |
| --- | --- |
| `q` | Case-insensitive substring of run or event names; SQL wildcard characters are literal |
| `status` | `running`, `completed` or `failed` |
| `model` | Exact recorded `attributes.model` value |
| `tool` | Exact name of a `tool.call` event |
| `min_duration_ms` | Minimum recorded run duration |
| `min_event_duration_ms` | Minimum event duration; when combined with `tool`, both apply to the same event |
| `min_cost_usd` | Minimum sum of recorded, nonnegative `cost_usd` observations |
| `after`, `before` | RFC3339 run start times, inclusive lower and exclusive upper bounds |
| `limit`, `offset` | 1..1000 items (default 100); nonnegative offset (default 0) |

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
