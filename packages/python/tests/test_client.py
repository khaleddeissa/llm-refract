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
