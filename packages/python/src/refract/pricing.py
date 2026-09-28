"""Versioned price feeds and explicit invoice reconciliation; never invented rates."""

from __future__ import annotations

import csv
import io
import json
import math
import threading
import urllib.parse
import urllib.request
from collections.abc import Mapping
from datetime import UTC, datetime
from decimal import Decimal, InvalidOperation
from typing import Any


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("price feeds may not redirect")


class PriceCatalog:
    def __init__(self, document: dict):
        if (
            document.get("currency") != "USD"
            or not isinstance(document.get("version"), str)
            or not document["version"]
        ):
            raise ValueError("price catalog requires a version and USD currency")
        effective = datetime.fromisoformat(document["effective_at"].replace("Z", "+00:00"))
        if effective.tzinfo is None or effective > datetime.now(UTC):
            raise ValueError("price effective_at must be an aware past timestamp")
        models = document.get("models")
        if not isinstance(models, dict) or len(models) > 10000:
            raise ValueError("models must be a bounded provider/model mapping")
        self.document = json.loads(json.dumps(document, allow_nan=False))
        for name, rates in models.items():
            if not isinstance(name, str) or "/" not in name or not isinstance(rates, dict):
                raise ValueError("rate keys must be provider/model")
            for required in ("input_per_million", "output_per_million"):
                if required not in rates:
                    raise ValueError("input and output prices are required")
            for key, rate in rates.items():
                if (
                    key
                    not in {
                        "input_per_million",
                        "output_per_million",
                        "cache_read_per_million",
                        "cache_write_per_million",
                    }
                    or isinstance(rate, bool)
                    or not isinstance(rate, (int, float))
                    or not math.isfinite(rate)
                    or rate < 0
                ):
                    raise ValueError("prices must be known, finite, nonnegative USD rates")

    def provider(self, name: str) -> dict:
        prefix = name + "/"
        return {
            model[len(prefix) :]: dict(rates)
            for model, rates in self.document["models"].items()
            if model.startswith(prefix)
        }


class PriceFeed:
    """Refresh an operator-selected HTTPS catalog in the background.

    Failed refreshes retain the last validated catalog for max_age seconds, after which
    new cost estimates become unknown. Network work never runs inside generation capture.
    """

    def __init__(self, url: str, *, interval=3600, max_age=86400, timeout=10, fetch=None):
        parsed = urllib.parse.urlsplit(url)
        if (
            parsed.scheme != "https"
            or not parsed.hostname
            or parsed.username
            or parsed.password
            or parsed.fragment
        ):
            raise ValueError("price feed must be a credential-free HTTPS URL")
        if not 0 < interval <= max_age or not 0 < timeout <= 60:
            raise ValueError("invalid refresh interval, maximum age or timeout")
        self.url, self.interval, self.max_age, self.timeout = url, interval, max_age, timeout
        self._fetch = fetch or self._download
        self._catalog: PriceCatalog | None = None
        self._updated = 0.0
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.last_error: str | None = None

    def _download(self) -> bytes:
        request = urllib.request.Request(self.url, headers={"Accept": "application/json"})
        with urllib.request.build_opener(_NoRedirect).open(
            request, timeout=self.timeout
        ) as response:
            body = response.read(1024 * 1024 + 1)
        if len(body) > 1024 * 1024:
            raise ValueError("price catalog exceeds 1 MiB")
        return body

    def refresh(self) -> bool:
        import time

        try:
            catalog = PriceCatalog(json.loads(self._fetch()))
            with self._lock:
                self._catalog, self._updated = catalog, time.monotonic()
                self.last_error = None
            return True
        except Exception as error:
            self.last_error = type(error).__name__
            return False

    def pricing(self, provider: str) -> Mapping:
        feed = self

        class CurrentPrices(Mapping):
            def _snapshot(self):
                import time

                with feed._lock:
                    if feed._catalog is None or time.monotonic() - feed._updated > feed.max_age:
                        return {}
                    return feed._catalog.provider(provider)

            def __getitem__(self, model):
                return self._snapshot()[model]

            def __iter__(self):
                return iter(self._snapshot())

            def __len__(self):
                return len(self._snapshot())

        return CurrentPrices()

    def start(self):
        if self._thread is not None:
            raise RuntimeError("price feeds are single-use")

        def work():
            while not self._stop.is_set():
                self.refresh()
                self._stop.wait(self.interval)

        self._thread = threading.Thread(target=work, daemon=True, name="refract-prices")
        self._thread.start()
        return self

    def close(self):
        self._stop.set()
        if self._thread:
            self._thread.join(self.timeout + 1)


def reconcile(recordings: list[dict], invoice_csv: str) -> dict[str, Any]:
    """Match normalized invoice lines to run_id/event_id, retaining unmatched rows.

    Required columns: run_id,event_id,currency,amount. Normalize provider-specific invoice
    exports explicitly before using this function; it cannot infer billing identifiers.
    """
    estimates = {}
    for recording in recordings:
        for event in recording["events"]:
            identity = (recording["id"], event["id"])
            if identity in estimates:
                raise ValueError("duplicate recorded event identity")
            cost = event.get("attributes", {}).get("cost_usd")
            if cost is not None and (
                isinstance(cost, bool)
                or not isinstance(cost, (int, float))
                or not math.isfinite(cost)
                or cost < 0
            ):
                raise ValueError("invalid recorded cost")
            estimates[identity] = None if cost is None else Decimal(str(cost))
    reader = csv.DictReader(io.StringIO(invoice_csv))
    if not {"run_id", "event_id", "currency", "amount"}.issubset(reader.fieldnames or []):
        raise ValueError("invoice requires run_id,event_id,currency,amount columns")
    actual: dict[tuple[str, str], Decimal] = {}
    for row in reader:
        if row["currency"] != "USD":
            raise ValueError("normalize invoice currency to USD explicitly")
        try:
            amount = Decimal(row["amount"])
        except InvalidOperation as error:
            raise ValueError("invalid invoice amount") from error
        if not amount.is_finite():
            raise ValueError("invoice amounts must be finite")
        identity = row["run_id"], row["event_id"]
        actual[identity] = actual.get(identity, Decimal(0)) + amount
    matched = []
    for identity in sorted(estimates.keys() & actual.keys()):
        estimate = estimates[identity]
        matched.append(
            {
                "run_id": identity[0],
                "event_id": identity[1],
                "estimated_usd": None if estimate is None else str(estimate),
                "billed_usd": str(actual[identity]),
                "difference_usd": None if estimate is None else str(actual[identity] - estimate),
            }
        )
    return {
        "currency": "USD",
        "matched": matched,
        "unmatched_invoice": [
            {"run_id": key[0], "event_id": key[1], "billed_usd": str(actual[key])}
            for key in sorted(actual.keys() - estimates.keys())
        ],
        "unbilled_events": [
            {"run_id": key[0], "event_id": key[1]}
            for key in sorted(estimates.keys() - actual.keys())
        ],
    }
