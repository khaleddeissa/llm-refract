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


def test_semantic_options_are_arguments_and_report_is_preserved():
    with patch.object(
        module.subprocess,
        "run",
        side_effect=[
            CompletedProcess([], 0),
            CompletedProcess([], 0),
            CompletedProcess([], 0, '{"passed":true,"differences":[]}', ""),
        ],
    ) as command:
        result = module.compare(
            Path("old.rfr"), Path("new.rfr"), "refract", {"similarity_threshold": 0.8}
        )
    assert result["passed"]
    assert command.call_args.args[0][-3:] == ["--semantic", "--threshold", "0.8"]
