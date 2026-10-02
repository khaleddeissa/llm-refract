import hashlib
import hmac
import json
from concurrent.futures import ThreadPoolExecutor

import pytest

from refract.delivery import WebhookInbox

SECRET = "local-contract-only"
SCOPE = dict(organization="org", project="project", environment="test")


def message(id="first", version=1, operation="put", **extra):
    body = json.dumps(
        dict(
            id=id,
            version=version,
            operation=operation,
            run_id="run",
            scope=SCOPE,
            payload="stored",
            **extra,
        )
    ).encode()
    return body, "sha256=" + hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest(), id


def accept(inbox, args):
    body, signature, id = args
    return inbox.accept(body, signature=signature, delivery_id=id)


def test_inbox_commit_restart_out_of_order_delete_and_authentication(tmp_path):
    path = tmp_path / "inbox.db"
    inbox = WebhookInbox(path, secret=SECRET)
    first = message()
    assert accept(inbox, first) == "applied"
    inbox.close()
    inbox = WebhookInbox(path, secret=SECRET)
    assert accept(inbox, first) == "duplicate"
    assert inbox.get("run", **SCOPE) == "stored"
    assert inbox.get("run", **{**SCOPE, "project": "other"}) is None
    assert accept(inbox, message("deleted", 3, "delete")) == "applied"
    assert accept(inbox, message("late", 2)) == "stale"
    assert inbox.get("run", **SCOPE) is None
    with pytest.raises(ValueError, match="signature"):
        inbox.accept(first[0] + b" ", signature=first[1], delivery_id=first[2])
    with pytest.raises(ValueError, match="different content"):
        accept(inbox, message("first", 4))
    assert accept(inbox, message("next", 4)) == "applied"  # previous transaction rolled back
    inbox.close()
    assert path.stat().st_mode & 0o777 == 0o600


def test_two_receivers_share_atomic_duplicate_receipts(tmp_path):
    a = WebhookInbox(tmp_path / "inbox.db", secret=SECRET)
    b = WebhookInbox(tmp_path / "inbox.db", secret=SECRET)
    with ThreadPoolExecutor(2) as pool:
        results = list(pool.map(lambda inbox: accept(inbox, message()), [a, b]))
    assert sorted(results) == ["applied", "duplicate"]
    a.close()
    b.close()
