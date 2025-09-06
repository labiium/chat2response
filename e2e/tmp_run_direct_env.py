#!/usr/bin/env python3
"""
tmp_run_direct_env.py

Helper script to:
1) Start a local mock Responses API server that implements /v1/responses.
2) Generate a temporary .env with OPENAI_BASE_URL pointing to the mock, a fake OPENAI_API_KEY,
   and MODEL=labiium-001 (you can change below).
3) Run the official runner in direct mode using that .env, printing STDOUT/STDERR and exit code.

Usage:
  python chat2response/e2e/tmp_run_direct_env.py

Requirements:
- Python 3.9+
- The file chat2response/e2e/run_official_responses_via_chat2response.py exists in this repo.
- The openai package is installed (the direct mode uses the OpenAI SDK).
"""

import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class _MockResponsesHandler(BaseHTTPRequestHandler):
    server_version = "MockResponses/0.1"

    def do_POST(self):
        # Only handle the Responses API endpoint
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

        # Build a minimal, plausible Responses API response; echo back the request for visibility
        resp_json = {
            "id": "resp_mock_cli",
            "object": "response",
            "model": (req_json.get("model") if isinstance(req_json, dict) else "unknown"),
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "ok (mock cli)"}
                    ],
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
        # Silence noisy default logs in tests
        return


def _pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _start_mock_server() -> tuple[ThreadingHTTPServer, int]:
    port = _pick_free_port()
    httpd = ThreadingHTTPServer(("127.0.0.1", port), _MockResponsesHandler)
    thr = threading.Thread(target=httpd.serve_forever, daemon=True)
    thr.start()
    return httpd, port


def main() -> int:
    # 1) Start mock upstream
    httpd, port = _start_mock_server()
    base_url = f"http://127.0.0.1:{port}/v1"
    print(f"[mock] Responses API listening on {base_url}")

    # 2) Create temporary .env for the runner
    tmpdir = tempfile.mkdtemp(prefix="run_cli_env_")
    env_path = Path(tmpdir) / ".env"

    # Customize these defaults if needed:
    model = "labiium-001"
    prompt = "Hello from .env test (direct mode)"

    env_text = f"""OPENAI_API_KEY=sk-test
OPENAI_BASE_URL={base_url}
MODEL={model}
PROMPT={prompt}
STREAM=false
"""
    env_path.write_text(env_text, encoding="utf-8")
    print(f"[env] Wrote {env_path}")

    # 3) Locate the runner and execute direct mode using the .env
    # Assumes this script lives at chat2response/e2e/tmp_run_direct_env.py
    here = Path(__file__).resolve()
    runner = (here.parent / "run_official_responses_via_chat2response.py").resolve()
    if not runner.is_file():
        print(f"[error] Runner not found at {runner}", file=sys.stderr)
        httpd.shutdown()
        httpd.server_close()
        return 2

    cmd = [
        sys.executable,
        str(runner),
        "--mode", "direct",
        "--env-file", str(env_path),
    ]
    print("[run]", " ".join(cmd))

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            cwd=str(Path.cwd()),
        )
        print("\n--- STDOUT ---")
        print(result.stdout)
        print("--- STDERR ---")
        print(result.stderr, file=sys.stderr)
        print(f"[exit] code={result.returncode}")
        return result.returncode
    finally:
        # 4) Shutdown mock server
        httpd.shutdown()
        httpd.server_close()


if __name__ == "__main__":
    sys.exit(main())
