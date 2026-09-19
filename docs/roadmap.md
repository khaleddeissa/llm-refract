# Foundation status and next milestones

The current slice implements the local artifact lifecycle, with tests and Docker packaging.

| Milestone | Remaining work                                                                                                                                                             |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Finish P0 | OTLP protobuf HTTP ingestion, incremental trace assembly, capture policies/sampling, richer schemas and graph validation, timeline tests across browsers                   |
| P1        | PostgreSQL adapter, S3 blobs, provider/framework adapters, approved hybrid executors, continuation from checkpoints, remote regression datasets and approved MCP mutations |
| P2        | Authentication/authorization, organizations, encrypted exports, retention, Kubernetes and deployment targets                                                               |
| P3        | Distributed replay, WASM, graph scale and causal analysis                                                                                                                  |

Draft interfaces and READMEs deliberately identify unimplemented work. A directory is not evidence of
production readiness. Provider adapters should wait until the execution and artifact contracts stabilize.
Release publishing requires confirmed package ownership, trusted publishers, signed images, SBOMs and
protected GitHub environments. The checked-in workflows only build and test release candidates.
