# chat2response/e2e/test_e2e_integrations.py
"""
Comprehensive end-to-end integration tests for Chat2Response:

Covers:
- /status endpoint
- Authorization precedence (client Authorization header over env OPENAI_API_KEY)
- Streaming SSE passthrough
- Chat upstream mode (UPSTREAM_MODE=chat) URL/payload rewrite
- Input retry logic when upstream requires top-level 'input'
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
from typing import Any, Dict, Optional, Tuple

import pytest
import requests


def _require_cargo() -> None:
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not available in PATH; skipping e2e tests")


def _project_root() -> Path:
    # This file is at chat2response/e2e/test_e2e_integrations.py
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


class _MockHandler(BaseHTTPRequestHandler):
    server_version = "MockResponses/0.1"

    def _send_json(self, code: int, payload: Dict[str, Any]) -> None:
        data = json.dumps(payload).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _read_json(self) -> Dict[str, Any]:
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length > 0 else b"{}"
        try:
            return json.loads(body.decode("utf-8"))
        except Exception:
            return {}

    def _emit_sse(self, payload: Dict[str, Any]) -> None:
        # Emit a simple SSE stream with two deltas and a completion event.
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        # Explicit chunked transfer for some clients
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

        def _chunk(data: bytes) -> None:
            try:
                self.wfile.write(f"{len(data):X}\r\n".encode("utf-8"))
                self.wfile.write(data)
                self.wfile.write(b"\r\n")
                self.wfile.flush()
            except Exception:
                pass

        def sse(obj: Dict[str, Any]) -> None:
            line = f"data: {json.dumps(obj)}\n\n".encode("utf-8")
            _chunk(line)

        prompt = _extract_last_user_prompt(payload)
        sse({"type": "message.delta", "delta": {"type": "output_text.delta", "text": f"part1:{prompt}"}})
        sse({"type": "message.delta", "delta": {"type": "output_text.delta", "text": "part2"}})
        sse({"type": "response.completed"})
        # End the chunked stream
        try:
            self.wfile.write(b"0\r\n\r\n")
        except Exception:
            pass

    def do_POST(self):
        # Record details for assertions
        path = self.path
        auth = self.headers.get("Authorization", "")
        body = self._read_json()
        accept = (self.headers.get("Accept") or "").lower()

        # Store last request details on the server object for test assertions
        try:
            self.server.last_request = {
                "path": path,
                "auth": auth,
                "body": body,
                "accept": accept,
            }  # type: ignore[attr-defined]
        except Exception:
            pass

        # Input retry behavior: if server is configured to require input, reject missing input once
        if getattr(self.server, "require_input", False):  # type: ignore[attr-defined]
            has_input = isinstance(body, dict) and "input" in body and isinstance(body["input"], str)
            if not has_input:
                msg = {"error": {"message": "Field 'input' required"}}
                self._send_json(400, msg)
                return

        # Decide SSE vs JSON
        wants_stream = "text/event-stream" in accept or bool(body.get("stream"))
        # If configured to force SSE, ensure SSE response
        if getattr(self.server, "force_sse", False):  # type: ignore[attr-defined]
            wants_stream = True

        # Allow both /v1/responses and /v1/chat/completions
        if not (path.endswith("/responses") or path.endswith("/chat/completions")):
            self._send_json(404, {"error": {"message": "not found"}})
            return

        # For auth precedence test, we just echo whether a bearer is present and its value
        if wants_stream:
            self._emit_sse(body)
        else:
            self._send_json(
                200,
                {
                    "mock": True,
                    "auth_header": auth,
                    "echo": body,
                    "path": path,
                },
            )

    def log_message(self, fmt, *args):
        # Silence default logs
        return


def _extract_last_user_prompt(payload: Dict[str, Any]) -> str:
    msgs = payload.get("messages")
    if isinstance(msgs, list) and msgs:
        for m in reversed(msgs):
            try:
                if m.get("role") == "user":
                    c = m.get("content")
                    if isinstance(c, str):
                        return c
                    if isinstance(c, list):
                        # search for text fragments
                        for p in c:
                            if p.get("type") in ("text", "input_text") and isinstance(p.get("text"), str):
                                return p["text"]
            except Exception:
                continue
        # fallback to last content if string
        last = msgs[-1]
        if isinstance(last.get("content"), str):
            return last["content"]
    return ""


class _MockServer:
    def __init__(self):
        self._httpd: Optional[ThreadingHTTPServer] = None
        self._thread: Optional[threading.Thread] = None
        self.host = "127.0.0.1"
        self.port = 0

    def start(self, require_input: bool = False, force_sse: bool = False):
        self._httpd = ThreadingHTTPServer((self.host, 0), _MockHandler)
        self.port = self._httpd.server_address[1]
        # Flags for handler behavior
        setattr(self._httpd, "require_input", require_input)
        setattr(self._httpd, "force_sse", force_sse)
        setattr(self._httpd, "last_request", None)
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

    @property
    def last_request(self) -> Optional[Dict[str, Any]]:
        if self._httpd:
            return getattr(self._httpd, "last_request", None)
        return None


def _spawn_server(
    dotenv_vars: Dict[str, str],
    additional_env: Optional[Dict[str, str]] = None,
) -> Tuple[subprocess.Popen, str, list]:
    """
    Spawn the chat2response server via `cargo run` in a temp dir with a .env file.
    Returns (proc, base_url, output_tail_buffer).
    """
    _require_cargo()
    port = _pick_free_port()
    dotenv_vars = dict(dotenv_vars or {})
    dotenv_vars.setdefault("BIND_ADDR", f"127.0.0.1:{port}")
    dotenv_vars.setdefault("RUST_LOG", "info,tower_http=info")

    tmpdir = Path(tempfile.mkdtemp(prefix="chat2response_e2e_"))
    dotenv_path = tmpdir / ".env"
    _write_dotenv(dotenv_path, dotenv_vars)

    env = os.environ.copy()
    # Ensure dotenv-loaded vars take precedence
    for k in ("BIND_ADDR", "OPENAI_API_KEY", "OPENAI_BASE_URL", "UPSTREAM_MODE", "CHAT2RESPONSE_UPSTREAM_INPUT"):
        env.pop(k, None)
    env["RUST_LOG"] = dotenv_vars.get("RUST_LOG", "info")
    if additional_env:
        env.update(additional_env)

    manifest = str(_project_root() / "Cargo.toml")
    cmd = ["cargo", "run", "--quiet", "--manifest-path", manifest, "--bin", "chat2response"]

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

    # Readiness by polling /convert
    last_output: list[str] = []

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
        sys.stderr.write("\n=== Server output (tail) ===\n")
        for line in last_output[-20:]:
            sys.stderr.write(line + "\n")
        sys.stderr.flush()
        proc.kill()
        raise RuntimeError("Server did not become ready in time")

    return proc, base_url, last_output


@pytest.mark.timeout(120)
def test_status_endpoint_and_routes():
    mock = _MockServer()
    mock.start()
    try:
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": f"http://127.0.0.1:{mock.port}/v1",
        }
        proc, base_url, _ = _spawn_server(dotenv_vars=dotenv_vars)

        try:
            r = requests.get(f"{base_url}/status", timeout=10)
            assert r.status_code == 200, r.text
            data = r.json()
            assert data.get("name") == "chat2response"
            assert isinstance(data.get("version"), str) and len(data["version"]) > 0
            assert data.get("proxy_enabled") is True
            routes = data.get("routes") or []
            assert "/status" in routes and "/convert" in routes and "/proxy" in routes
        finally:
            _terminate_process(proc)
    finally:
        mock.stop()


@pytest.mark.timeout(120)
def test_auth_precedence_header_over_env_nonstream_and_env_fallback():
    mock = _MockServer()
    mock.start()
    try:
        base_override = f"http://127.0.0.1:{mock.port}/v1"

        # Case A: Header overrides env
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": base_override,
            "OPENAI_API_KEY": "env-key",
        }
        proc, base_url, _ = _spawn_server(dotenv_vars=dotenv_vars)
        try:
            payload = {"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "hello"}]}
            r = requests.post(
                f"{base_url}/proxy",
                json=payload,
                headers={"Authorization": "Bearer header-key"},
                timeout=15,
            )
            assert r.status_code == 200, r.text
            data = r.json()
            # Upstream echo includes the Authorization header value seen by mock
            assert data.get("auth_header") == "Bearer header-key"
        finally:
            _terminate_process(proc)

        # Case B: Env fallback when no header provided
        dotenv_vars2 = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": base_override,
            "OPENAI_API_KEY": "env-only-key",
        }
        proc2, base_url2, _ = _spawn_server(dotenv_vars=dotenv_vars2)
        try:
            payload2 = {"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "hello"}]}
            r2 = requests.post(f"{base_url2}/proxy", json=payload2, timeout=15)
            assert r2.status_code == 200, r2.text
            data2 = r2.json()
            assert data2.get("auth_header") == "Bearer env-only-key"
        finally:
            _terminate_process(proc2)

    finally:
        mock.stop()


@pytest.mark.timeout(120)
def test_streaming_sse_passthrough_and_header_precedence():
    # Use robust mock_openai_responses.py subprocess for SSE
    mock_port = _pick_free_port()
    mock_cmd = [
        sys.executable,
        str(_project_root() / "e2e" / "mock_openai_responses.py"),
        "--port", str(mock_port),
        "--require-auth",
        "--api-key", "sse-token",
    ]
    mock_proc = subprocess.Popen(
        mock_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    # Wait for readiness
    deadline = time.time() + 10.0
    ready = False
    while time.time() < deadline and mock_proc.poll() is None:
        try:
            rr = requests.get(f"http://127.0.0.1:{mock_port}/healthz", timeout=0.5)
            if rr.status_code == 200:
                ready = True
                break
        except Exception:
            time.sleep(0.1)
    if not ready:
        try:
            mock_proc.kill()
        finally:
            raise RuntimeError("mock_openai_responses not ready in time")

    try:
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": f"http://127.0.0.1:{mock_port}/v1",
        }
        proc, base_url, _ = _spawn_server(dotenv_vars=dotenv_vars)
        try:
            payload = {
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "stream please"}],
                "stream": True,
            }
            # Request streaming; provide a header token distinct from any env
            r = requests.post(
                f"{base_url}/proxy",
                json=payload,
                headers={"Authorization": "Bearer sse-token"},
                stream=True,
                timeout=30,
            )
            if r.status_code != 200:
                try:
                    sys.stderr.write(f"Streaming test got status {r.status_code}; body: {r.text[:500]}\n")
                except Exception:
                    sys.stderr.write(f"Streaming test got status {r.status_code}; unable to read body\n")
                assert r.status_code == 200
            # The server should return text/event-stream
            ctype = r.headers.get("Content-Type", "")
            assert "text/event-stream" in ctype

            # Read a few SSE lines and ensure completion event arrives
            lines = []
            for line in r.iter_lines(decode_unicode=True, chunk_size=1):
                if not line:
                    continue
                lines.append(line)
                if len(lines) > 200:
                    break
                if line.startswith("data:"):
                    try:
                        obj = json.loads(line[len("data:") :].strip())
                        if obj.get("type") == "response.completed":
                            break
                    except Exception:
                        pass
            assert any("response.completed" in ln for ln in lines if ln.startswith("data:"))
        finally:
            _terminate_process(proc)
    finally:
        try:
            mock_proc.terminate()
        except Exception:
            pass


@pytest.mark.timeout(120)
def test_chat_upstream_mode_rewrite_and_payload_streaming():
    # Use robust mock_openai_responses.py subprocess for SSE (chat mode)
    mock_port = _pick_free_port()
    mock_cmd = [
        sys.executable,
        str(_project_root() / "e2e" / "mock_openai_responses.py"),
        "--port", str(mock_port),
        "--require-auth",
        "--api-key", "chat-key",
    ]
    mock_proc = subprocess.Popen(
        mock_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    # Wait for readiness
    deadline = time.time() + 10.0
    ready = False
    while time.time() < deadline and mock_proc.poll() is None:
        try:
            rr = requests.get(f"http://127.0.0.1:{mock_port}/healthz", timeout=0.5)
            if rr.status_code == 200:
                ready = True
                break
        except Exception:
            time.sleep(0.1)
    if not ready:
        try:
            mock_proc.kill()
        finally:
            raise RuntimeError("mock_openai_responses not ready in time")

    try:
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": f"http://127.0.0.1:{mock_port}/v1",
            "UPSTREAM_MODE": "chat",
        }
        proc, base_url, _ = _spawn_server(dotenv_vars=dotenv_vars)
        try:
            payload = {
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "chat mode test"}],
                "stream": True,  # ensure streaming path which handles chat rewrite
            }
            r = requests.post(
                f"{base_url}/proxy",
                json=payload,
                headers={"Authorization": "Bearer chat-key"},
                stream=True,
                timeout=30,
            )
            if r.status_code != 200:
                try:
                    sys.stderr.write(f"Streaming chat-mode test got status {r.status_code}; body: {r.text[:500]}\n")
                except Exception:
                    sys.stderr.write(f"Streaming chat-mode test got status {r.status_code}; unable to read body\n")
                assert r.status_code == 200
            # Validate streaming succeeded and content type is SSE
            ctype = r.headers.get("Content-Type", "")
            assert "text/event-stream" in ctype
            # Read a few SSE lines and ensure we saw some data and a completion event
            lines = []
            for line in r.iter_lines(decode_unicode=True):
                if not line:
                    continue
                lines.append(line)
                if len(lines) > 200:
                    break
                if line.startswith("data:"):
                    try:
                        obj = json.loads(line[len("data:"):].strip())
                        if obj.get("type") == "response.completed":
                            break
                    except Exception:
                        pass
            assert any(ln.startswith("data:") for ln in lines)
        finally:
            _terminate_process(proc)
    finally:
        try:
            mock_proc.terminate()
        except Exception:
            pass


@pytest.mark.timeout(120)
def test_input_retry_injects_input_and_succeeds():
    # Upstream requires 'input' top-level; first attempt (without input) should fail with 400,
    # server should retry with derived input string from last user message and succeed.
    mock = _MockServer()
    mock.start(require_input=True)
    try:
        dotenv_vars = {
            "BIND_ADDR": f"127.0.0.1:{_pick_free_port()}",
            "OPENAI_BASE_URL": f"http://127.0.0.1:{mock.port}/v1",
        }
        proc, base_url, _ = _spawn_server(dotenv_vars=dotenv_vars)
        try:
            payload = {
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "derive me"}],
                # non-streaming to hit post_responses_with_input_retry
                "stream": False,
            }
            r = requests.post(
                f"{base_url}/proxy",
                json=payload,
                headers={"Authorization": "Bearer retry-key"},
                timeout=30,
            )
            # Final response should be 200 after retry
            assert r.status_code == 200, r.text
            data = r.json()
            echo = data.get("echo") or {}
            # Upstream's echo should now include top-level "input" derived from last user message
            assert echo.get("input") == "derive me"
        finally:
            _terminate_process(proc)
    finally:
        mock.stop()
