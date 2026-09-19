import json
from pathlib import Path

import pytest

from refract.artifact import pack, unpack


def test_text_roundtrip_and_tamper_detection():
    run = json.loads(
        (
            Path(__file__).resolve().parents[3] / "tests/fixtures/simple-run/execution.json"
        ).read_text()
    )
    data = pack(run)
    assert data.decode("utf-8").startswith('{"format":')
    assert unpack(data) == run
    assert pack(run) == data
    with pytest.raises(ValueError, match="checksum"):
        unpack(data.replace(b"demo-1", b"demo-2"))
    with pytest.raises(ValueError, match="header"):
        unpack(b"invalid")
