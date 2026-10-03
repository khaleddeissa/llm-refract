"""Authenticated service client for project embeddings and execution search."""

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class RefractClient:
    def __init__(self, url: str = "http://127.0.0.1:8000", *, api_key: str | None = None):
        parsed = urllib.parse.urlsplit(url)
        if (
            parsed.scheme not in {"http", "https"}
            or not parsed.hostname
            or parsed.username
            or parsed.password
        ):
            raise ValueError("service URL requires HTTP(S), a host, and no credentials")
        if parsed.query or parsed.fragment:
            raise ValueError("service URL must not contain query or fragment")
        self.url = url.rstrip("/")
        self.api_key = api_key
        self._opener = urllib.request.build_opener(_NoRedirect())

    def request(
        self, path: str, body: Any = None, *, method: str | None = None, timeout: float = 35
    ) -> Any:
        if not path.startswith("/v1/"):
            raise ValueError("request path must start with /v1/")
        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["Authorization"] = "Bearer " + self.api_key
        request = urllib.request.Request(
            self.url + path,
            data=None if body is None else json.dumps(body, allow_nan=False).encode(),
            headers=headers,
            method=method,
        )
        with self._opener.open(request, timeout=timeout) as response:
            payload = response.read(17 * 1024 * 1024 + 1)
        if len(payload) > 17 * 1024 * 1024:
            raise ValueError("service response exceeds 17 MiB")
        return json.loads(payload)

    def embedding_models(self) -> list[dict]:
        return self.request("/v1/embedding-models")["models"]

    def embedding_settings(self) -> dict:
        return self.request("/v1/project/embeddings")

    def configure_embeddings(self, profiles: list[dict]) -> dict:
        """Admin only. Enable operator-approved profiles and queue existing runs."""
        return self.request("/v1/admin/project/embeddings", profiles, method="PUT")

    def reindex_embeddings(self) -> dict:
        return self.request("/v1/admin/embeddings/reindex", {})

    def search_text(
        self, query: str, *, profile: str | None = None, limit: int = 20, mode: str = "auto"
    ) -> dict:
        """Generate a query vector with a selected project model, then search this tenant."""
        return self.request(
            "/v1/search/text", {"query": query, "profile": profile, "limit": limit, "mode": mode}
        )

    def search_vector(
        self, model: str, values: list[float], *, limit: int = 20, mode: str = "auto"
    ) -> list[dict]:
        return self.request(
            "/v1/search/vector",
            {"embedding": {"model": model, "values": values}, "limit": limit, "mode": mode},
        )

    def generation_models(self) -> list[dict]:
        return self.request("/v1/generation-models")["models"]

    def rerun(
        self,
        run_id: str,
        *,
        profile: str,
        from_event: str,
        allow_live: bool = False,
        reuse_recorded: list[str] | None = None,
        approved_events: list[str] | None = None,
    ) -> dict:
        """Create a provider-executed branch. Named reused steps never execute application tools."""
        if not run_id or run_id in {".", ".."}:
            raise ValueError("invalid run id")
        return self.request(
            "/v1/runs/" + urllib.parse.quote(run_id, safe="") + "/rerun",
            {
                "profile": profile,
                "from_event": from_event,
                "allow_live": allow_live,
                "reuse_recorded": reuse_recorded or [],
                "approved_events": approved_events or [],
            },
            timeout=130,
        )

    def compare(
        self,
        left: str,
        right: str,
        *,
        semantic: bool = True,
        grader: str | None = None,
        allow_live: bool = False,
        options: dict | None = None,
    ) -> dict:
        return self.request(
            "/v1/diff",
            {
                "left": left,
                "right": right,
                "semantic": semantic,
                "grader": grader,
                "allow_live": allow_live,
                "options": options or {},
            },
            timeout=130,
        )

    def telemetry(
        self, *, kind: str = "logs", trace_id: str = "", limit: int = 50, offset: int = 0
    ) -> dict:
        return self.request(
            "/v1/telemetry?"
            + urllib.parse.urlencode(
                {"kind": kind, "trace_id": trace_id, "limit": limit, "offset": offset}
            )
        )
