# chat2response/e2e/test_e2e_openai_client.py
"""
End-to-end tests for the OpenAI Python client using the Responses API, targeting a mock upstream.

This file:
- Starts a local mock HTTP server implementing the OpenAI Responses endpoint (/v1/responses).
- Uses the official `openai` Python package client to call `client.responses.create(...)`.
- Verifies request shape (Authorization header, JSON body) and asserts the echoed payload.

Notes:
- These tests are fully offline and do not contact OpenAI.
- If the `openai` package is not installed, the tests are skipped.
- We use the client's `with_raw_response` to avoid strict schema coupling across versions.
"""

import json
import socket
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Optional

import pytest

try:
    # OpenAI Python SDK v1.x
    from openai import OpenAI  # type: ignore
    HAVE_OPENAI = True
except Exception:
    HAVE_OPENAI = False


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class _MockResponsesHandler(BaseHTTPRequestHandler):
    server_version = "MockResponses/0.1"

    def do_POST(self):
        # Accept only the Responses API path
        if not self.path.endswith("/responses"):
            self.send_response(404)
            self.end_headers()
            return

        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length > 0 else b"{}"
        try:
            req_json = json.loads(body.decode("utf-8"))
        except Exception:
            req_json = None

        # Validate bearer token presence (value doesn't matter in tests)
        auth = self.headers.get("Authorization", "")
        auth_header_present = auth.startswith("Bearer ")

        # Build a minimal, plausible Responses API response object.
        # We include a "mock" flag and echo the inbound for verification.
        # Using minimal schema to reduce coupling with SDK versions.
        resp_json = {
            "id": "resp_mock_123",
            "object": "response",
            "model": req_json.get("model") if isinstance(req_json, dict) else "unknown",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "ok (mock)"}
                    ],
                }
            ],
            "mock": True,
            "auth_header_present": auth_header_present,
            "echo": req_json,
        }

        resp_bytes = json.dumps(resp_json).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp_bytes)))
        self.end_headers()
        self.wfile.write(resp_bytes)

    def log_message(self, fmt, *args):
        # Silence the default server logs to keep test output clean
        return


class _MockServer:
    def __init__(self):
        self._httpd: Optional[ThreadingHTTPServer] = None
        self._thread: Optional[threading.Thread] = None
        self.host = "127.0.0.1"
        self.port = 0

    def start(self):
        self._httpd = ThreadingHTTPServer((self.host, 0), _MockResponsesHandler)
        self.port = self._httpd.server_address[1]
        self._thread = threading.Thread(target=self._httpd.serve_forever, daemon=True)
        self._thread.start()

    def stop(self):
        if self._httpd:
            try:
                self._httpd.shutdown()
            except Exception:
                pass
            try:
                self._httpd.server_close()
            except Exception:
                pass
        if self._thread:
            try:
                self._thread.join(timeout=5.0)
            except Exception:
                pass


@pytest.mark.skipif(not HAVE_OPENAI, reason="openai package not installed")
@pytest.mark.timeout(30)
def test_openai_responses_create_non_streaming_to_mock():
    """
    Validate that the OpenAI Python client can POST to a mock Responses endpoint and
    retrieve the upstream response using `with_raw_response` to avoid SDK schema coupling.
    """
    mock = _MockServer()
    mock.start()
    try:
        base_url = f"http://127.0.0.1:{mock.port}/v1"
        client = OpenAI(base_url=base_url, api_key="sk-test")

        # Prefer raw response for version resiliency across SDK changes.
        # If with_raw_response is unavailable in this SDK version, fall back and unpack.
        raw_supported = hasattr(client.responses, "with_raw_response")
        if raw_supported:
            resp = client.responses.with_raw_response.create(
                model="gpt-4o-mini",
                input="Hello via OpenAI client (mock)",
            )
            assert hasattr(resp, "status_code"), "raw response missing status_code"
            assert resp.status_code == 200
            # Try robust parsing across SDK variants:
            if hasattr(resp, "parse"):
                obj = resp.parse()
                if hasattr(obj, "model_dump"):
                    data = obj.model_dump()
                elif hasattr(obj, "to_dict"):
                    data = obj.to_dict()
                elif hasattr(obj, "dict"):
                    data = obj.dict()
                elif hasattr(obj, "model_dump_json"):
                    data = json.loads(obj.model_dump_json())
                else:
                    data = json.loads(json.dumps(obj, default=lambda o: getattr(o, "__dict__", str(o))))
            else:
                # Fallback to raw bytes -> JSON
                raw = resp.read()
                text = raw.decode("utf-8") if isinstance(raw, (bytes, bytearray)) else str(raw)
                data = json.loads(text)
        else:
            # Fallback: rely on the typed object and turn into dict conservatively
            obj = client.responses.create(
                model="gpt-4o-mini",
                input="Hello via OpenAI client (mock)",
            )
            # Try common attribute conversion methods across SDK versions
            if hasattr(obj, "model_dump"):
                data = obj.model_dump()
            elif hasattr(obj, "to_dict"):
                data = obj.to_dict()
            elif hasattr(obj, "dict"):
                data = obj.dict()  # type: ignore
            else:
                # As a last resort, serialize then parse
                if hasattr(obj, "model_dump_json"):
                    data = json.loads(obj.model_dump_json())
                else:
                    data = json.loads(json.dumps(obj, default=lambda o: getattr(o, "__dict__", str(o))))

        # Assertions on the mock response shape
        assert data.get("mock") is True
        assert data.get("auth_header_present") is True
        assert data.get("object") == "response"
        assert data.get("model") == "gpt-4o-mini"
        assert isinstance(data.get("output"), list)
        assert data.get("echo", {}).get("input") == "Hello via OpenAI client (mock)"
    finally:
        mock.stop()
