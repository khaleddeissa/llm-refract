# Execution inspector

The inspector is the browser interface bundled with the Docker image. It reads the same REST API
used by the SDKs and MCP. Start the service with `docker compose up --build -d --wait`, open
<http://localhost:8000>, and submit an execution using an [SDK](python.md) or the
[HTTP example](../../examples/http/ingest.py). See [Docker usage](docker.md) for storage and networking.

## Workspace and empty forks

Choose a run in the left sidebar. The summary shows its status, event count, summed event duration,
start time and replay mode. An empty fork is valid: forking before the first event preserves zero
events. It does not indicate that a model is executing in the background.

![Inspector workspace showing a selected empty fork and no recorded events](../../assets/Inspector_Layout_1.PNG)

This screenshot shows an empty prefix branch. The event inspector has nothing to display until a run
with recorded events is selected; the running status is stored metadata, not a live worker indicator.

## Inspect a tool call

Select an event in the execution timeline to view its recorded input, output and attributes.
The example below selects `lookup`, a completed `tool.call` whose output is `{"found": true}`.
A `null` input means no input was recorded; a zero duration means the recording reports no elapsed time.

![Selected lookup tool call with its recorded JSON output in the event inspector](../../assets/Inspector_Layout_2.PNG)

## Follow a multi-step execution

The timeline preserves event order. This example records retrieval followed by generation; selecting
`Draft answer` displays its prompt, captured answer, provider/model attributes, parent event and replay
policy. The duration bars help compare recorded step durations; their sum is not necessarily wall-clock
runtime when events overlap.

![Retrieval and generation timeline with the selected answer, parent event and recorded replay policy](../../assets/Inspector_Layout_3.PNG)

## Replay, branch, compare and export

1. **Replay recorded** returns captured outputs. It does not call a model or execute a tool.
2. **Fork before event** creates a stored prefix ending before the selected event. It preserves lineage
   but does not resume your application. Select a populated run and an event to enable it.
3. **Compare with…**, then **Diff**, compares the selected run with another recorded run. To test new
   application behavior, first record a fresh execution and then compare it with the baseline.
4. **Export .rfr** downloads a readable, checksummed execution artifact. Open it in a text editor or use
   `refract inspect` / `refract validate`; see the [format guide](artifacts.md).

These screenshots illustrate recorded example data, not a hosted demo. The UI currently has no
built-in authentication; see [production boundaries](../production.md) before sharing a deployment.
