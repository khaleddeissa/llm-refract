# Signed delivery and durable inbox

Both receivers verify the original request bytes and atomically commit a receipt plus the newest
scoped payload/tombstone. They acknowledge only after commit. Restarting and resending a delivery
returns `duplicate`; a delayed lower version returns `stale`. Neither repeats a business side effect.

```bash
export REFRACT_WEBHOOK_SECRET=local-demo-secret-change-in-production
export REFRACT_INBOX_PATH=/tmp/refract-inbox.sqlite3
uv run python examples/delivery/receiver.py
# Or, after npm run build --workspace @llm-refract/sdk:
node examples/delivery/receiver.mjs
```

Run one receiver at a time, then start Refract with the same secret and
`REFRACT_WEBHOOK_URL=http://127.0.0.1:8091`. Create a run, stop/restart the receiver, and trigger more
deliveries. Run retention sends a higher-version deletion; resending an old PUT cannot restore it.
The stored payload is the service's original JSON string or encrypted envelope. The inbox does not
need decryption keys to mirror it.

These HTTP examples bind loopback for local tests. Use your production HTTP framework, HTTPS, request
body/time limits, encrypted volumes and managed secrets when deploying. Python uses stdlib SQLite;
Node's `openWebhookInbox` uses `node:sqlite` (Node 22.13+ without flags, Node 24 recommended). SQLite
receipt/payload writes are atomic; external email/payment/webhook effects still need a transactional
outbox or the downstream system's idempotency mechanism. Keep receipts and version tombstones while
old deliveries could be retried. Restore the inbox and service backups together.

The tests in `packages/python/tests/test_delivery.py` and
`packages/typescript/tests/delivery.test.ts` deliberately reorder versions, duplicate receipts, tamper
with signatures and restart the inbox. Version numbers are assigned by the service database, not the
caller or wall clock.
