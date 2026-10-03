"""Send synthetic OTLP logs and metrics, then read normalized records."""

import os
import time

from refract import RefractClient

client = RefractClient(
    os.environ.get("REFRACT_SERVER_URL", "http://127.0.0.1:8000"),
    api_key=os.environ.get("REFRACT_API_KEY"),
)
now = str(time.time_ns())
client.request(
    "/v1/logs",
    {
        "resourceLogs": [
            {
                "scopeLogs": [
                    {
                        "logRecords": [
                            {
                                "timeUnixNano": now,
                                "severityText": "INFO",
                                "body": {"stringValue": "Local example generation completed"},
                                "attributes": [
                                    {"key": "api_key", "value": {"stringValue": "redacted-fixture"}}
                                ],
                            }
                        ]
                    }
                ]
            }
        ]
    },
)
client.request(
    "/v1/metrics",
    {
        "resourceMetrics": [
            {
                "scopeMetrics": [
                    {
                        "metrics": [
                            {
                                "name": "example.requests",
                                "unit": "{request}",
                                "sum": {
                                    "aggregationTemporality": 2,
                                    "isMonotonic": True,
                                    "dataPoints": [{"timeUnixNano": now, "asInt": "1"}],
                                },
                            }
                        ]
                    }
                ]
            }
        ]
    },
)
print(client.telemetry(kind="logs", limit=1))
print(client.telemetry(kind="metrics", limit=1))
