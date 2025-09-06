# chat2response/e2e/test_e2e_chat_compat.py
"""
End-to-end proxy test using /proxy and a mock Responses API upstream.

Flow:
1) Start a local mock upstream that implements /v1/responses (Responses API).
2) Start chat2response with the "proxy" feature, configured via a pseudo .env to point
   OPENAI_BASE_URL at the mock upstream and to bind to a free local port.
3) POST a Chat Completions request to this server’s /proxy.
4) Assert the upstream mock received a converted Responses payload and returned the expected JSON.

Notes:
- This test runs fully offline; the mock upstream stands in for OpenAI.
- Skips automatically if cargo is not available.
"""

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import pytest
import requests


def _require_cargo() -> None:
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not available in PATH; skipping e2e tests")


def _project_root() -> Path:
    # This file lives at chat2response/e2e/test_e2e_chat_compat.py
    # The Cargo.toml is at chat2response/Cargo.toml (one level up).
    here = Path(__file__).resolve()
    return here.parents[1]


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _write_dotenv(path: Path, variables: Dict[str, str]) -> None:
    lines = [f"{k}={v}" for k, v in variables.items()]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _spawn_server(
    features: Optional[List[str]],
    dotenv_vars: Dict[str, str],
    additional_env: Optional[Dict[str, str]] = None,
) -> Tuple[subprocess.Popen, str]:
    """
    Spawn the chat2response HTTP server using `cargo run`, loading configuration
    from a pseudo .env file in a temp directory.

    - features: list of cargo features (e.g., ["proxy"]) or None
    - dotenv_vars: dict of environment variables to write into .env
    - additional_env: extra environment vars for the child process

    Returns (process, base_url) where base_url is "http://127.0.0.1:PORT".
    """
    _require_cargo()

    # Fixed port via BIND_ADDR for predictability
    port = _pick_free_port()
    dotenv_vars = dict(dotenv_vars or {})
    dotenv_vars.setdefault("BIND_ADDR", f"127.0.0.1:{port}")
    dotenv_vars.setdefault("RUST_LOG", "info,tower_http=info")

    tmpdir = Path(tempfile.mkdtemp(prefix="chat2response_e2e_"))
    dotenv_path = tmpdir / ".env"
    _write_dotenv(dotenv_path, dotenv_vars)

    env = os.environ.copy()
    # Ensure dotenv-loaded vars take precedence
    for k in ("BIND_ADDR", "OPENAI_API_KEY", "OPENAI_BASE_URL"):
        env.pop(k, None)
    env["RUST_LOG"] = dotenv_vars.get("RUST_LOG", "info")
    # Force HTTP mode (not MCP)
    env.pop("CHAT2RESPONSE_MCP", None)
    if additional_env:
        env.update(additional_env)

    manifest = str(_project_root() / "Cargo.toml")
    cmd = ["cargo", "run", "--quiet", "--manifest-path", manifest, "--bin", "chat2response"]
    if features:
        cmd.extend(["--features", ",".join(features)])

    proc = subprocess.Popen(
        cmd,
        cwd=str(tmpdir),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
        text=True,
        bufsize=1,
    )

    base_url = f"http://{dotenv_vars['BIND_ADDR']}"

    # Readiness: poll /convert until 200/400/422 or process exits
    last_output = []

    def _pump_output():
        try:
            assert proc.stdout is not None
            for line in proc.stdout:
                if not line:
                    break
                last_output.append(line.rstrip())
                if len(last_output) > 50:
                    del last_output[0 : len(last_output) - 50]
        except Exception:
            pass

    t = threading.Thread(target=_pump_output, daemon=True)
    t.start()

    ready_payload = {
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    }
    deadline = time.time() + 60.0
    ready = False
    while time.time() < deadline and proc.poll() is None:
        try:
            r = requests.post(f"{base_url}/convert", json=ready_payload, timeout=1.0)
            if r.status_code in (200, 400, 422):
                ready = True
                break
        except Exception:
            time.sleep(0.2)

    if not ready:
        sys.stderr.write("\n=== chat2response output (tail) ===\n")
        for line in last_output[-20:]:
            sys.stderr.write(line + "\n")
        sys.stderr.flush()
        try:
            proc.kill()
        except Exception:
            pass
        raise RuntimeError("Server did not become ready in time")

    return proc, base_url


def _terminate_process(proc: subprocess.Popen, timeout: float = 10.0) -> None:
    if proc.poll() is not None:
        return
    try:
        if os.name == "nt":
            proc.terminate()
        else:
            proc.send_signal(signal.SIGTERM)
        deadline = time.time() + timeout
        while time.time() < deadline and proc.poll() is None:
            time.sleep(0.1)
        if proc.poll() is None:
            proc.kill()
    except Exception:
        try:
            proc.kill()
        except Exception:
            pass


class _MockResponsesHandler(BaseHTTPRequestHandler):
    server_version = "MockResponses/0.1"

    def do_POST(self):
        # Only handle Responses API
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

        auth = self.headers.get("Authorization", "")
        auth_present = auth.startswith("Bearer ")

        # Build plausible Responses response; echo the request for assertions
        resp_json = {
            "id": "resp_mock_001",
            "object": "response",
            "model": req_json.get("model") if isinstance(req_json, dict) else "unknown",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "ok (mock upstream)"}],
                }
            ],
            "mock": True,
            "auth_header_present": auth_present,
            "echo": req_json,
        }
        data = json.dumps(resp_json).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, fmt, *args):
        # Silence default logs
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


@ pytest.mark.timeout(60)
def test_proxy_via_requests_to_responses():
    """
    Send a Chat Completions request to /proxy and verify the mock Responses upstream receives
    a converted Responses payload and returns a plausible Responses response.
    """
    # Start mock upstream (implements /v1/responses)
    mock = _MockServer()
    mock.start()
    try:
        # Configure server via pseudo .env
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_API_KEY": "sk-test",
            "OPENAI_BASE_URL": f"http://127.0.0.1:{mock.port}/v1",
        }
        # Start server (proxy always enabled)
        proc, server_base = _spawn_server(features=None, dotenv_vars=dotenv_vars)

        try:
            payload = {
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Hello via ChatCompat"}],
            }
            resp = requests.post(f"{server_base}/proxy", json=payload, timeout=15)
            assert resp.status_code == 200, resp.text
            data = resp.json()

            # Assertions on the mock upstream response returned by our server
            assert data.get("mock") is True
            assert data.get("auth_header_present") is True
            assert data.get("object") == "response"
            assert data.get("model") == "gpt-4o-mini"

            # The upstream echo is the converted Responses payload (messages form)
            echo = data.get("echo", {})
            assert isinstance(echo.get("messages"), list)
            assert echo["messages"][0]["role"] == "user"  # preserved by conversion
            content = echo["messages"][0]["content"]
            if isinstance(content, str):
                assert content == "Hello via ChatCompat"
        finally:
            _terminate_process(proc)

    finally:
        mock.stop()
