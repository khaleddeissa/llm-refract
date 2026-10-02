import io
import json
from unittest.mock import Mock

import pytest

from refract import RefractClient


def test_semantic_client_sends_auth_and_project_selection():
    client = RefractClient(api_key="test-key")
    opener = Mock()
    opener.open.return_value = io.BytesIO(b'{"runs": [], "matches": []}')
    client._opener = opener
    assert client.search_text("refund", profile="local")["runs"] == []
    request = opener.open.call_args.args[0]
    assert request.get_header("Authorization") == "Bearer test-key"
    assert request.full_url == "http://127.0.0.1:8000/v1/search/text"
    assert json.loads(request.data)["profile"] == "local"
    opener.open.return_value = io.BytesIO(b'{"profiles": []}')
    client.configure_embeddings([])
    assert opener.open.call_args.args[0].get_method() == "PUT"
    with pytest.raises(ValueError):
        client.request("https://untrusted.invalid")


def test_service_client_rejects_credential_urls_and_nonfinite_vectors():
    for url in ["file:///tmp/run", "https://secret@example.invalid", "https://example.invalid#x"]:
        with pytest.raises(ValueError):
            RefractClient(url)
    with pytest.raises(ValueError):
        RefractClient().search_vector("test", [float("nan")])


def test_replay_and_grading_clients_preserve_explicit_consent_and_bounded_timeout():
    client = RefractClient()
    opener = Mock()
    client._opener = opener
    opener.open.return_value = io.BytesIO(b'{"id":"branch"}')
    assert client.rerun("a/b", profile="local", from_event="first")["id"] == "branch"
    request = opener.open.call_args.args[0]
    assert request.full_url.endswith("/v1/runs/a%2Fb/rerun")
    assert json.loads(request.data)["allow_live"] is False
    assert opener.open.call_args.kwargs["timeout"] == 130
    opener.open.return_value = io.BytesIO(b"{}")
    client.compare("a", "b", grader="domain", allow_live=True)
    assert json.loads(opener.open.call_args.args[0].data)["grader"] == "domain"
