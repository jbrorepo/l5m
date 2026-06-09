"""Dependency-free L5M HTTP client (stdlib only).

Keeping this on `urllib` means there is no third-party supply chain to vet — a
deliberate choice for a security product. If you prefer `requests`, the surface
is small enough to port in a few minutes.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Dict, Iterable, List, Optional


class L5mError(Exception):
    """Base error. `status` is the HTTP status (None for transport failures)."""

    def __init__(self, message: str, status: Optional[int] = None) -> None:
        super().__init__(message)
        self.status = status


class AuthError(L5mError):
    """401/403 — missing or invalid credentials."""


class RateLimited(L5mError):
    """429 — per-tenant rate limit exceeded; back off and retry."""


class Client:
    """A small, synchronous client for the L5M server.

    Provide EITHER ``api_key`` (+ ``tenant_id`` and optional masks) for header
    auth, OR ``bearer_token`` for JWT auth where the principal is derived from
    verified claims server-side.
    """

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
        bearer_token: Optional[str] = None,
        tenant_id: Optional[int] = None,
        context_mask: str = "0xffff",
        policy_mask: str = "0xffff",
        trust_floor: int = 0,
        timeout: float = 10.0,
    ) -> None:
        if not bearer_token and tenant_id is None:
            raise ValueError("provide tenant_id (header auth) or bearer_token (JWT auth)")
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.bearer_token = bearer_token
        self.tenant_id = tenant_id
        self.context_mask = context_mask
        self.policy_mask = policy_mask
        self.trust_floor = trust_floor
        self.timeout = timeout

    # -- internals ---------------------------------------------------------
    def _headers(self) -> Dict[str, str]:
        h = {"content-type": "application/json"}
        if self.bearer_token:
            h["authorization"] = f"Bearer {self.bearer_token}"
        else:
            if self.api_key:
                h["x-l5m-api-key"] = self.api_key
            h["x-l5m-tenant"] = str(self.tenant_id)
            h["x-l5m-context"] = self.context_mask
            h["x-l5m-policy"] = self.policy_mask
            h["x-l5m-trust"] = str(self.trust_floor)
        return h

    def _request(self, method: str, path: str, body: Optional[Any] = None) -> Any:
        url = f"{self.base_url}{path}"
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(url, data=data, method=method, headers=self._headers())
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read()
                if not raw:
                    return None
                ctype = resp.headers.get("content-type", "")
                if "application/json" in ctype:
                    return json.loads(raw)
                return raw.decode("utf-8")
        except urllib.error.HTTPError as e:
            detail = e.read().decode("utf-8", "replace")
            msg = f"{method} {path} -> {e.code}: {detail}"
            if e.code in (401, 403):
                raise AuthError(msg, e.code) from None
            if e.code == 429:
                raise RateLimited(msg, e.code) from None
            raise L5mError(msg, e.code) from None
        except urllib.error.URLError as e:
            raise L5mError(f"{method} {path} failed: {e.reason}") from None

    # -- public API --------------------------------------------------------
    def insert(self, capsule: Dict[str, Any]) -> Dict[str, Any]:
        """Insert/update a memory. The server forces tenant ownership regardless
        of any ``tenant_id`` in the body."""
        return self._request("POST", "/v1/memories", capsule)

    def insert_many(self, capsules: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
        """Insert several memories. The server has no batch endpoint yet, so this
        issues one request per capsule and returns each response."""
        return [self.insert(c) for c in capsules]

    def query(
        self,
        text: str,
        *,
        max_capsules: int = 8,
        as_of: Optional[int] = None,
        mode: Optional[str] = None,
        embedding: Optional[List[float]] = None,
    ) -> Dict[str, Any]:
        """Run a gated retrieval. Returns the full server response (frame +
        coverage + metadata)."""
        body: Dict[str, Any] = {"query": text, "max_capsules": max_capsules}
        if as_of is not None:
            body["as_of"] = as_of
        if mode is not None:
            body["mode"] = mode
        if embedding is not None:
            body["embedding"] = embedding
        return self._request("POST", "/v1/query", body)

    def delete(self, capsule_id: int | str) -> Dict[str, Any]:
        """Hide a memory from all future results (delete / supersede)."""
        return self._request("DELETE", f"/v1/memories/{capsule_id}")

    def verify_audit(self) -> Dict[str, Any]:
        """Verify the tamper-evident audit chain. Returns
        ``{"intact": true, "verified": N}`` when the hash chain is unbroken."""
        return self._request("GET", "/v1/audit/verify")

    def metrics(self) -> str:
        """Fetch the Prometheus metrics exposition (text)."""
        return self._request("GET", "/metrics")

    def healthz(self) -> bool:
        """True if the server is up."""
        try:
            self._request("GET", "/healthz")
            return True
        except L5mError:
            return False
