import importlib.util
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("compare", Path(__file__).parents[1] / "compare.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def test_detects_changed_execution():
    with patch.object(
        module.subprocess,
        "run",
        side_effect=[
            CompletedProcess([], 0),
            CompletedProcess([], 0),
            CompletedProcess([], 1, '[{"index":1}]', ""),
        ],
    ):
        result = module.compare(Path("old.rfr"), Path("new.rfr"), "refract")
    assert not result["passed"]
    assert result["differences"][0]["index"] == 1
