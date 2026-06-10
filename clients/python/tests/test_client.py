"""Client tests against an in-process stub server (stdlib http.server).

No Rust binary needed — the stub asserts the client sends the right headers,
method, path, and body, and that it maps status codes to the right exceptions.
"""

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from l5m import AuthError, Client, L5mError, RateLimited  # noqa: E402

# Captures what the last request looked like, for assertions.
LAST = {}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):  # silence
        pass

    def _capture(self):
        length = int(self.headers.get("content-length", 0))
        raw = self.rfile.read(length) if length else b""
        LAST.clear()
        LAST.update(
            method=self.command,
            path=self.path,
            headers={k.lower(): v for k, v in self.headers.items()},
            body=json.loads(raw) if raw else None,
        )

    def _send(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self._capture()
        if self.path == "/healthz":
            self.send_response(200)
            self.send_header("content-length", "2")
            self.end_headers()
            self.wfile.write(b"ok")
        elif self.path == "/metrics":
            body = b"l5m_queries_total 3\n"
            self.send_response(200)
            self.send_header("content-type", "text/plain; version=0.0.4")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path == "/v1/audit/verify":
            self._send(200, {"intact": True, "verified": 2})
        elif self.path == "/v1/usage":
            self._send(
                200,
                {
                    "tenants": [
                        {
                            "tenant": 1,
                            "queries": 2,
                            "capsules_returned": 4,
                            "inserts": 1,
                            "deletes": 0,
                        }
                    ]
                },
            )
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self):
        self._capture()
        # Simulate auth + rate-limit behavior for assertions.
        if LAST["headers"].get("x-l5m-api-key") == "wrong":
            self._send(401, {"error": "unauthorized"})
        elif LAST["headers"].get("x-l5m-tenant") == "999":
            self._send(429, {"error": "rate limit exceeded"})
        elif self.path == "/v1/memories":
            self._send(201, {"status": "inserted"})
        elif self.path == "/v1/query":
            self._send(200, {"frame": {"capsules": [{"score": 1.0, "claim": "hi"}]}})
        else:
            self._send(404, {"error": "not found"})

    def do_DELETE(self):
        self._capture()
        self._send(200, {"status": "deleted"})


@pytest.fixture(scope="module")
def server():
    httpd = HTTPServer(("127.0.0.1", 0), Handler)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    host, port = httpd.server_address
    yield f"http://{host}:{port}"
    httpd.shutdown()


def client(server, **kw):
    kw.setdefault("api_key", "secret")
    kw.setdefault("tenant_id", 7)
    return Client(server, **kw)


def test_requires_tenant_or_bearer(server):
    with pytest.raises(ValueError):
        Client(server)  # neither tenant_id nor bearer_token


def test_insert_sends_headers_and_body(server):
    c = client(server)
    out = c.insert({"capsule_id": "1", "claim": "x"})
    assert out == {"status": "inserted"}
    assert LAST["method"] == "POST"
    assert LAST["path"] == "/v1/memories"
    assert LAST["headers"]["x-l5m-api-key"] == "secret"
    assert LAST["headers"]["x-l5m-tenant"] == "7"
    assert LAST["body"]["claim"] == "x"


def test_query_returns_frame(server):
    c = client(server)
    resp = c.query("hello", max_capsules=3)
    assert resp["frame"]["capsules"][0]["claim"] == "hi"
    assert LAST["body"] == {"query": "hello", "max_capsules": 3}


def test_bearer_auth_sets_authorization(server):
    c = Client(server, bearer_token="tok.tok.tok")
    c.query("hi")
    assert LAST["headers"]["authorization"] == "Bearer tok.tok.tok"
    assert "x-l5m-tenant" not in LAST["headers"]


def test_delete(server):
    c = client(server)
    assert c.delete(42) == {"status": "deleted"}
    assert LAST["method"] == "DELETE"
    assert LAST["path"] == "/v1/memories/42"


def test_auth_error_maps_401(server):
    c = client(server, api_key="wrong")
    with pytest.raises(AuthError):
        c.insert({"capsule_id": "1"})


def test_rate_limited_maps_429(server):
    c = client(server, tenant_id=999)
    with pytest.raises(RateLimited):
        c.query("hi")


def test_usage_returns_metering_rows(server):
    rows = client(server).usage()
    assert rows[0]["tenant"] == 1
    assert rows[0]["queries"] == 2


def test_verify_audit(server):
    c = client(server)
    assert c.verify_audit() == {"intact": True, "verified": 2}


def test_metrics_is_text(server):
    c = client(server)
    assert "l5m_queries_total" in c.metrics()


def test_healthz(server):
    assert client(server).healthz() is True


def test_unreachable_raises_l5merror():
    c = Client("http://127.0.0.1:1", api_key="secret", tenant_id=1, timeout=1.0)
    with pytest.raises(L5mError):
        c.query("hi")
