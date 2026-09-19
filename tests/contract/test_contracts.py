import json
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource

import refract

ROOT = Path(__file__).resolve().parents[2]


def validator():
    directory = ROOT / "spec/execution/v1"
    event = json.loads((directory / "event.schema.json").read_text())
    execution = json.loads((directory / "execution.schema.json").read_text())
    registry = Registry().with_resource(event["$id"], Resource.from_contents(event))
    return Draft202012Validator(execution, registry=registry, format_checker=FormatChecker())


def test_shared_fixture():
    fixture = json.loads((ROOT / "tests/fixtures/simple-run/execution.json").read_text())
    validator().validate(fixture)


def test_python_sdk_matches_wire_contract():
    with refract.run("contract") as run:
        refract.event(
            type="state.change", name="discount", input={"total": 120}, output={"total": 90}
        )
    validator().validate(run.snapshot())
