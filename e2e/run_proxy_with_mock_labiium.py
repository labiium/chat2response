#!/usr/bin/env python3
"""
run_proxy_with_mock_labiium.py

Start a local mock Responses API that supports both JSON and SSE responses,
spawn the Chat2Response server configured to proxy to this mock,
then POST a /proxy request with model "labiium-001" and optionally stream the output.

Requirements
- Python 3.9+
- requests
- cargo (to spawn the Rust server)
- This script must be run from within the repository (or any location where Cargo.toml is accessible).

Usage examples
1) Non-streaming (default):
   python chat2response/e2e/run_proxy_with_mock_labiium.py

2) Streaming:
   python chat2response/e2e/run_proxy_with_mock_labiium.py --stream --prompt "Stream please"

3) Use a custom prompt and keep server alive after request:
   python chat2response/e2e/run_proxy_with_mock_labiium.py --prompt "Hello" --keep-alive

4) Attach to an already-running Chat2Response server (ensure it points to the mock):
   python chat2response/e2e/run_proxy_with_mock_labiium.py --attach-base http://127.0.0.1:8099

Notes
- By default, this script spawns the Chat2Response server with the "proxy" feature and a temporary .env:
  BIND_ADDR: the chosen free port
  OPENAI_BASE_URL: http://127.0.0.1:<mock_port>/v1
  OPENAI_API_KEY: "sk-test"
- The mock upstream responds with SSE when request JSON has "stream": true, and with JSON otherwise.
- The model used is "labiium-001" by default to avoid hitting OpenAI.

Exit codes
- 0 on success; non-zero on failures.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Optional

import requests


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class _MockResponsesHandler(BaseHTTPRequestHandler):
    server_version = "MockResponsesSSE/0.1"

    def do_POST(self):
        # Expect /v1/responses
        if not self.path.endswith("/responses"):
            self.send_response(404)
            self.end_headers()
            return

        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length > 0 else b"{}"

        try:
            req_json = json.loads(body.decode("utf-8"))
        except Exception:
            req_json = {}

        # If "stream": true => respond with SSE
        stream = False
        try:
            stream = bool(req_json.get("stream", False))
        except Exception:
            stream = False

        if stream:
            # Minimal SSE illustrating a streaming output path
            sse_bytes = (
                b"event: response.created\n"
                b"data: {\"type\":\"response.created\"}\n\n"
                b"event: response.output_text.delta\n"
                b"data: {\"delta\":\"Hello from labiium-001 (SSE)\"}\n\n"
                b"event: response.completed\n"
                b"data: {\"type\":\"response.completed\"}\n\n"
            )
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(sse_bytes)))
            self.end_headers()
            self.wfile.write(sse_bytes)
            return

        # Otherwise: non-streaming JSON echo
        resp_json = {
            "mock": True,
            "message": "Hello from mock Responses (non-streaming)",
            "echo": req_json,
        }
        resp_bytes = json.dumps(resp_json).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp_bytes)))
        self.end_headers()
        self.wfile.write(resp_bytes)

    def log_message(self, fmt, *args):
        # Silence base HTTP logs
        return


class _MockServer:
    def __init__(self) -> None:
        self._httpd: Optional[ThreadingHTTPServer] = None
        self._thr: Optional[threading.Thread] = None
        self.host = "127.0.0.1"
        self.port = 0

    def start(self) -> None:
        self._httpd = ThreadingHTTPServer((self.host, 0), _MockResponsesHandler)
        self.port = self._httpd.server_address[1]
        self._thr = threading.Thread(target=self._httpd.serve_forever, daemon=True)
        self._thr.start()

    def stop(self) -> None:
        if self._httpd is not None:
            try:
                self._httpd.shutdown()
            except Exception:
                pass
            try:
                self._httpd.server_close()
            except Exception:
                pass
        if self._thr is not None:
            try:
                self._thr.join(timeout=3.0)
            except Exception:
                pass


def _spawn_chat2response(base_port: int, mock_port: int) -> tuple[subprocess.Popen[str], str, list[str]]:
    """
    Spawn Chat2Response with proxy feature enabled, binding to base_port and
    pointing OPENAI_BASE_URL to the mock upstream.

    Returns: (process, base_url, log_tail_list)
    """
    # Determine project root: this file is at chat2response/e2e/...
    here = Path(__file__).resolve()
    project_root = here.parents[1]
    cargo_toml = project_root / "Cargo.toml"

    tmpdir = Path(tempfile.mkdtemp(prefix="c2r_run_mock_"))
    env_file = tmpdir / ".env"
    env_file.write_text(
        "\n".join(
            [
                f"BIND_ADDR=127.0.0.1:{base_port}",
                "RUST_LOG=info,tower_http=info",
                "OPENAI_API_KEY=sk-test",
                f"OPENAI_BASE_URL=http://127.0.0.1:{mock_port}/v1",
            ]
        )
        + "\n",
        encoding="utf-8",
    )

    env = os.environ.copy()
    # Ensure our .env is used and not overridden by parent env
    env.pop("BIND_ADDR", None)
    env.pop("OPENAI_API_KEY", None)
    env.pop("OPENAI_BASE_URL", None)
    env.pop("CHAT2RESPONSE_MCP", None)

    cmd = [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(cargo_toml),
        "--bin",
        "chat2response",
    ]
    proc = subprocess.Popen(
        cmd,
        cwd=str(tmpdir),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        env=env,
    )

    logs: list[str] = []

    def _pump():
        try:
            assert proc.stdout is not None
            for line in proc.stdout:
                if not line:
                    break
                logs.append(line.rstrip())
                if len(logs) > 200:
                    del logs[:100]
        except Exception:
            pass

    t = threading.Thread(target=_pump, daemon=True)
    t.start()

    base_url = f"http://127.0.0.1:{base_port}"
    # Wait for readiness
    deadline = time.time() + 60.0
    ready = False
    while time.time() < deadline and proc.poll() is None:
        try:
            r = requests.get(base_url + "/status", timeout=0.75)
            if r.status_code in (200, 400, 422):
                ready = True
                break
        except Exception:
            time.sleep(0.25)
    if not ready:
        sys.stderr.write("\n=== chat2response (tail) ===\n")
        for l in logs[-40:]:
            sys.stderr.write(l + "\n")
        sys.stderr.flush()
        try:
            proc.terminate()
        except Exception:
            pass
        raise RuntimeError("chat2response did not become ready in time")

    return proc, base_url, logs


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description="Run /proxy against a local mock Responses API using model labiium-001 (optionally stream).")
    p.add_argument("--prompt", default="Stream please", help="User prompt to send.")
    p.add_argument("--model", default="labiium-001", help="Model name to send to /proxy.")
    p.add_argument("--stream", action="store_true", help="Enable streaming SSE path.")
    p.add_argument("--keep-alive", action="store_true", help="Keep the spawned chat2response server alive after the request.")
    p.add_argument("--attach-base", default=None, help="Attach to an existing Chat2Response base URL (skip spawning). WARNING: the server must already be configured to proxy to the mock upstream started by this script.")
    args = p.parse_args(argv)

    mock = _MockServer()
    mock.start()
    mock_base = f"http://127.0.0.1:{mock.port}/v1"
    sys.stderr.write(f"[mock] Responses API listening on {mock_base}\n")

    proc: Optional[subprocess.Popen] = None
    base_url: str

    try:
        if args.attach_base:
            base_url = args.attach_base.rstrip("/")
            sys.stderr.write(f"[info] Attaching to existing Chat2Response at {base_url}\n")
            sys.stderr.write("[warn] Ensure your server has OPENAI_BASE_URL pointing to the mock above.\n")
        else:
            # Spawn server wired to mock
            server_port = _pick_free_port()
            proc, base_url, _ = _spawn_chat2response(server_port, mock.port)
            sys.stderr.write(f"[ok] Chat2Response ready at {base_url}\n")

        payload = {
            "model": args.model,
            "messages": [{"role": "user", "content": args.prompt}],
        }
        if args.stream:
            payload["stream"] = True  # type: ignore[assignment]

        url = f"{base_url}/proxy"
        sys.stderr.write(f"POST {url} (stream={args.stream}) ...\n")

        if args.stream:
            r = requests.post(url, json=payload, timeout=60, stream=True)
            r.raise_for_status()
            sys.stderr.write("Streaming response (SSE/raw):\n")
            for line in r.iter_lines(decode_unicode=True):
                if line is None:
                    continue
                print(line)
        else:
            r = requests.post(url, json=payload, timeout=30)
            r.raise_for_status()
            # Body may be application/json or raw text; try JSON first
            try:
                data = r.json()
            except Exception:
                data = r.text
            print(json.dumps({"proxied": data}, indent=2))

        if args.keep_alive and proc is not None:
            sys.stderr.write("[info] --keep-alive set; press Ctrl-C to stop the server.\n")
            try:
                while True:
                    time.sleep(1.0)
            except KeyboardInterrupt:
                pass

        return 0
    except requests.HTTPError as e:
        sys.stderr.write(f"HTTP error: {e}\n")
        if e.response is not None:
            try:
                sys.stderr.write(f"Response body: {e.response.text}\n")
            except Exception:
                pass
        return 1
    except Exception as e:
        sys.stderr.write(f"Error: {e}\n")
        return 2
    finally:
        if proc is not None and not args.keep_alive:
            try:
                if os.name == "nt":
                    proc.terminate()
                else:
                    proc.send_signal(signal.SIGTERM)
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
            except Exception:
                pass
        mock.stop()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
