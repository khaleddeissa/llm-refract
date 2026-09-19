"""Run after docker compose up; demonstrates SDK use of the Docker HTTP service."""

import os

import refract

with refract.run(
    "remote-example", endpoint=os.environ.get("REFRACT_SERVER_URL", "http://localhost:8000")
):
    refract.event(
        type="generation", name="demo answer", output={"text": "Hello from application code"}
    )
