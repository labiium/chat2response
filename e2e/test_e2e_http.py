# chat2response/e2e/test_e2e_http.py
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
    # This file is at chat2response/e2e/test_e2e_http.py
    # The Cargo.toml we want is at chat2response/Cargo.toml (parent of e2e).
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
    Spawn the chat2response HTTP server using `cargo run`, loading configuration from a pseudo .env.

    - features: Cargo features list (e.g., ["proxy"]) or None.
    - dotenv_vars: Variables to write into a temporary .env file.
    - additional_env: Extra environment entries to pass to the child process.

    Returns (process, base_url) where base_url is like "http://127.0.0.1:PORT".
    """
    _require_cargo()

    # Fixed port so we don't need to parse logs
    port = _pick_free_port()
    dotenv_vars = dict(dotenv_vars or {})
    dotenv_vars.setdefault("BIND_ADDR", f"127.0.0.1:{port}")
    # Default logging for readable startup message (optional)
    dotenv_vars.setdefault("RUST_LOG", "info,tower_http=info")

    tmpdir = Path(tempfile.mkdtemp(prefix="chat2response_e2e_"))
    dotenv_path = tmpdir / ".env"
    _write_dotenv(dotenv_path, dotenv_vars)

    env = os.environ.copy()
    # Ensure .env values take effect (dotenv loads first; we avoid clobbering with existing env)
    env.pop("BIND_ADDR", None)
    env.pop("OPENAI_API_KEY", None)
    env.pop("OPENAI_BASE_URL", None)
    env["RUST_LOG"] = dotenv_vars.get("RUST_LOG", "info")

    if additional_env:
        env.update(additional_env)

    manifest = str(_project_root() / "Cargo.toml")
    cmd = ["cargo", "run", "--quiet", "--manifest-path", manifest, "--bin", "chat2response"]
    # features are ignored; binary includes all capabilities

    # Run in temp dir so the process reads our .env
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

    # Wait for server readiness by polling /convert with a minimal valid request
    deadline = time.time() + 60.0
    last_output = []

    def _pump_output():
        # Capture first lines for debugging when readiness fails.
        try:
            for line in proc.stdout:
                if not line:
                    break
                last_output.append(line.rstrip())
                # Keep last 50 lines
                if len(last_output) > 50:
                    del last_output[0: len(last_output) - 50]
        except Exception:
            pass

    pump_thr = threading.Thread(target=_pump_output, daemon=True)
    pump_thr.start()

    ready_payload = {
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    }
    ready = False
    while time.time() < deadline and proc.poll() is None:
        try:
            r = requests.post(f"{base_url}/convert", json=ready_payload, timeout=1.0)
            # If server is up, either it responds 200 OK or 400/422 if schema mismatched.
            if r.status_code in (200, 400, 422):
                ready = True
                break
        except Exception:
            time.sleep(0.2)
    if not ready:
        # Print last output to aid debugging
        sys.stderr.write("\n=== Server output (tail) ===\n")
        for line in last_output[-20:]:
            sys.stderr.write(line + "\n")
        sys.stderr.flush()
        # Kill process if it didn't start
        proc.kill()
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
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length > 0 else b"{}"
        try:
            req_json = json.loads(body.decode("utf-8"))
        except Exception:
            req_json = None

        # Basic path check: we expect /v1/responses
        if not self.path.endswith("/responses"):
            self.send_response(404)
            self.end_headers()
            return

        # For visibility, we can validate bearer header (optional)
        auth = self.headers.get("Authorization", "")
        # Compose a mock response echoing back the request (non-streaming)
        resp_json = {"mock": True, "auth_header_present": auth.startswith("Bearer "), "echo": req_json}

        resp_bytes = json.dumps(resp_json).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp_bytes)))
        self.end_headers()
        self.wfile.write(resp_bytes)

    def log_message(self, format, *args):
        # Silence default HTTP server logs in tests
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


@pytest.mark.timeout(120)
def test_convert_endpoint_e2e():
    """
    End-to-end test for /convert:
    - Spawns the Rust server with pseudo .env and a fixed free port.
    - Posts a Chat Completions request.
    - Asserts that mapping to Responses payload is correct.
    """
    proc, base_url = _spawn_server(features=None, dotenv_vars={"BIND_ADDR": f"127.0.0.1:{_pick_free_port()}"})
    try:
        payload = {
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Say hi"},
            ],
            "max_tokens": 32,
            "temperature": 0.2,
            "stream": False,
        }
        r = requests.post(f"{base_url}/convert?conversation_id=conv-1", json=payload, timeout=10)
        assert r.status_code == 200, r.text
        data = r.json()
        # Top-level mappings
        assert data["model"] == "gpt-4o-mini"
        assert data["max_output_tokens"] == 32
        assert data.get("temperature") == 0.2
        assert data.get("stream") is False
        assert data.get("conversation") == "conv-1"
        # Messages preserved
        assert isinstance(data["messages"], list)
        assert data["messages"][0]["role"] == "system"
        assert data["messages"][1]["role"] == "user"
        assert data["messages"][0]["content"] == "You are helpful."
        assert data["messages"][1]["content"] == "Say hi"
    finally:
        _terminate_process(proc)


@pytest.mark.timeout(120)
def test_convert_multimodal_and_tools_e2e():
    """
    End-to-end test for /convert with multimodal content, response_format and tools.
    """
    proc, base_url = _spawn_server(features=None, dotenv_vars={"BIND_ADDR": f"127.0.0.1:{_pick_free_port()}"})
    try:
        payload = {
            "model": "gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Describe the image"},
                        {"type": "image_url", "image_url": {"url": "https://example.com/cat.png"}},
                    ],
                }
            ],
            "response_format": {"type": "json_object", "schema": {"type": "object"}},
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "description": "Lookup a value",
                        "parameters": {
                            "type": "object",
                            "properties": {"key": {"type": "string"}},
                            "required": ["key"],
                        },
                    },
                }
            ],
            "tool_choice": {"type": "function", "function": {"name": "lookup"}},
        }
        r = requests.post(f"{base_url}/convert", json=payload, timeout=10)
        assert r.status_code == 200, r.text
        data = r.json()
        # Response format forwarded
        assert data["response_format"]["type"] == "json_object"
        assert "schema" in data["response_format"]
        # Tools mapping
        assert isinstance(data.get("tools"), list) and len(data["tools"]) == 1
        tool = data["tools"][0]
        assert tool["type"] == "function"
        assert tool["function"]["name"] == "lookup"
        # Multimodal content preserved
        assert data["messages"][0]["content"][0]["type"] == "text"
        assert data["messages"][0]["content"][1]["type"] == "image_url"
        # Tool choice forwarded
        assert data["tool_choice"] == {"type": "function", "function": {"name": "lookup"}}
    finally:
        _terminate_process(proc)


@pytest.mark.timeout(120)
def test_proxy_endpoint_e2e_with_mock_upstream():
    """
    End-to-end test for /proxy with a mock upstream Responses API:
    - Starts a mock HTTP server implementing /v1/responses.
    - Spawns chat2response built with the "proxy" feature.
    - Configures pseudo .env with OPENAI_BASE_URL pointing to the mock; provides Authorization header on request.
    - Posts a request to /proxy and verifies the mock response is returned.
    """
    mock = _MockServer()
    mock.start()
    try:
        base_url_override = f"http://127.0.0.1:{mock.port}/v1"
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": base_url_override,
        }
        proc, base_url = _spawn_server(features=["proxy"], dotenv_vars=dotenv_vars)

        payload = {
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hello via proxy"}],
        }
        r = requests.post(f"{base_url}/proxy", json=payload, headers={"Authorization": "Bearer sk-test"}, timeout=15)
        assert r.status_code == 200, r.text
        # Body may not have JSON content-type header; parse explicitly
        body = r.content.decode("utf-8")
        data = json.loads(body)
        assert data.get("mock") is True
        assert data.get("auth_header_present") is True
        assert data["echo"]["model"] == "gpt-4o-mini"
        assert data["echo"]["messages"][0]["role"] == "user"
    finally:
        mock.stop()
        # Ensure server is terminated
        try:
            _terminate_process(proc)
        except Exception:
            pass
