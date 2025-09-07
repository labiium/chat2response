#!/usr/bin/env python3
"""
mock_openai_responses.py

A tiny mock of the OpenAI Responses API for local proxy/e2e testing.

Features
- POST /v1/responses:
  - If Accept: text/event-stream or body.stream == true → return SSE stream with a few events.
  - Else → return a compact JSON response echoing the prompt.
- GET /healthz → 200 OK (readiness probe)
- Optional auth checks (Bearer token). See --require-auth and --api-key.

No third-party dependencies; Python 3.9+.

Examples
- JSON mode:
    python e2e/mock_openai_responses.py --port 18080
    curl -sS http://127.0.0.1:18080/v1/responses \
      -H 'content-type: application/json' \
      -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}'

- Streaming mode:
    python e2e/mock_openai_responses.py --port 18080
    curl -N http://127.0.0.1:18080/v1/responses \
      -H 'content-type: application/json' \
      -H 'accept: text/event-stream' \
      -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Stream, please"}],"stream":true}'

- With auth:
    python e2e/mock_openai_responses.py --require-auth --api-key sk-test
    curl -sS http://127.0.0.1:18080/v1/responses \
      -H 'authorization: Bearer sk-test' \
      -H 'content-type: application/json' \
      -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello"}]}'
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import threading
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, Optional, Tuple


def _now_ms() -> int:
    return int(time.time() * 1000)


def _extract_last_user_prompt(payload: Dict[str, Any]) -> str:
    """
    Try to extract a user prompt from a Chat-like payload:
      { "messages": [ { "role": "user", "content": ... }, ... ] }
    Falls back to a generic greeting.
    """
    try:
        messages = payload.get("messages") or []
        for m in reversed(messages):
            if isinstance(m, dict) and str(m.get("role", "")).lower() == "user":
                c = m.get("content")
                if isinstance(c, str):
                    return c
                # If content is array-like multimodal, try to pick text
                if isinstance(c, list):
                    for part in c:
                        if isinstance(part, dict) and part.get("type") in ("text", "input_text"):
                            t = part.get("text") or part.get("content") or ""
                            if isinstance(t, str) and t.strip():
                                return t
                # Otherwise, just json-dump the content
                return json.dumps(c, ensure_ascii=False)
    except Exception:
        pass
    return "Hello from mock Responses!"


class MockResponsesHandler(BaseHTTPRequestHandler):
    server_version = "MockOpenAIResponses/0.1"
    protocol_version = "HTTP/1.1"

    # These are set via server (ThreadingHTTPServer) attributes
    # to avoid relying on globals.
    #   require_auth: bool
    #   api_key: Optional[str]
    #   sse_chunks: int
    #   sse_delay_ms: int
    #   json_delay_ms: int

    # Minimal logging: prefix with time and thread
    def log_message(self, fmt: str, *args: Any) -> None:
        ts = time.strftime("%H:%M:%S")
        tname = threading.current_thread().name
        sys.stderr.write(f"[{ts} {tname}] {self.address_string()} - {fmt % args}\n")

    def do_GET(self) -> None:
        if self.path == "/healthz":
            self._send_json(HTTPStatus.OK, {"status": "ok", "ts": _now_ms()})
            return
        self._send_json(HTTPStatus.NOT_FOUND, {"error": {"message": "Not found"}})

    def do_POST(self) -> None:
        if self.path not in ("/v1/responses", "/v1/chat/completions"):
            self._send_json(HTTPStatus.NOT_FOUND, {"error": {"message": "Not found"}})
            return

        # Auth check (optional)
        if getattr(self.server, "require_auth", False):  # type: ignore[attr-defined]
            if not self._check_auth():
                self._send_json(HTTPStatus.UNAUTHORIZED, {"error": {"message": "Missing or invalid Authorization"}})
                return

        # Read body
        length = int(self.headers.get("content-length", "0") or "0")
        raw = self.rfile.read(length) if length > 0 else b""
        try:
            payload = json.loads(raw.decode("utf-8")) if raw else {}
            if not isinstance(payload, dict):
                payload = {}
        except Exception:
            payload = {}

        # Streaming decision
        accept = (self.headers.get("accept") or "").lower()
        wants_stream = "text/event-stream" in accept or bool(payload.get("stream"))

        if wants_stream:
            delay_ms = int(getattr(self.server, "sse_delay_ms", 50))  # type: ignore[attr-defined]
            chunks = int(getattr(self.server, "sse_chunks", 3))       # type: ignore[attr-defined]
            self._handle_sse(payload, chunks, delay_ms)
        else:
            delay_ms = int(getattr(self.server, "json_delay_ms", 0))  # type: ignore[attr-defined]
            if delay_ms > 0:
                time.sleep(delay_ms / 1000.0)
            self._handle_json(payload)

    # ---- Helpers ----

    def _check_auth(self) -> bool:
        auth = self.headers.get("authorization") or self.headers.get("Authorization") or ""
        if not auth.startswith("Bearer "):
            return False
        supplied = auth[len("Bearer ") :].strip()
        expected = getattr(self.server, "api_key", None)  # type: ignore[attr-defined]
        if expected:
            return supplied == expected
        # If no explicit API key is configured, accept any non-empty Bearer token
        return len(supplied) > 0

    def _handle_json(self, payload: Dict[str, Any]) -> None:
        # Build a simple Responses-like JSON
        model = payload.get("model") or "gpt-4o-mini"
        prompt = _extract_last_user_prompt(payload)
        text = f"Echo (mock): {prompt}"

        rid = f"resp_{uuid.uuid4().hex[:12]}"
        body = {
            "id": rid,
            "model": model,
            "created": int(time.time()),
            "type": "response",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": text}
                    ],
                }
            ],
            "usage": {"input_tokens": 12, "output_tokens": len(text.split()), "total_tokens": 12 + len(text.split())},
        }
        self._send_json(HTTPStatus.OK, body)

    def _handle_sse(self, payload: Dict[str, Any], chunks: int, delay_ms: int) -> None:
        try:
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "keep-alive")
            # Some clients require an explicit transfer-encoding when streaming without length
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
        except BrokenPipeError:
            return

        # Emit a minimal sequence of typed SSE "data:" lines.
        # Align with common Responses streaming event "type" fields.
        def sse(obj: Dict[str, Any]) -> None:
            try:
                data = json.dumps(obj, ensure_ascii=False).encode("utf-8")
                # chunked transfer: write chunk size in hex + CRLF, then data + CRLF
                # Each SSE frame ends with double newline, so we include that in chunk.
                frame = b"data: " + data + b"\n\n"
                self.wfile.write(hex(len(frame))[2:].encode("ascii") + b"\r\n")
                self.wfile.write(frame + b"\r\n")
                self.wfile.flush()
            except BrokenPipeError:
                pass

        # Start
        sse({"type": "message_start", "created": int(time.time())})

        prompt = _extract_last_user_prompt(payload)
        out = f"Echo (stream mock): {prompt}"
        if chunks <= 0:
            chunks = 1
        step = max(1, len(out) // chunks)
        pos = 0
        while pos < len(out):
            frag = out[pos : pos + step]
            pos += step
            sse({"type": "response.output_text.delta", "delta": frag})
            if delay_ms > 0:
                time.sleep(delay_ms / 1000.0)

        # Done
        sse({"type": "response.completed"})
        # End chunked stream: 0-sized chunk
        try:
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        except BrokenPipeError:
            pass

    def _send_json(self, status: HTTPStatus, body: Dict[str, Any]) -> None:
        data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            self.wfile.flush()
        except BrokenPipeError:
            pass


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Mock OpenAI Responses upstream server (for local proxy testing)")
    p.add_argument("--host", default="127.0.0.1", help="Bind host (default: 127.0.0.1)")
    p.add_argument("--port", type=int, default=int(os.getenv("MOCK_OAI_PORT", "18080")), help="Bind port (default: 18080)")
    p.add_argument("--require-auth", action="store_true", default=bool_env("REQUIRE_AUTH", False),
                   help="Require Authorization: Bearer <token> header")
    p.add_argument("--api-key", default=os.getenv("OPENAI_API_KEY"),
                   help="Expected Bearer token value (default: env OPENAI_API_KEY if set; otherwise accept any non-empty token when --require-auth is used)")
    p.add_argument("--sse-chunks", type=int, default=int(os.getenv("SSE_CHUNKS", "3")),
                   help="Number of SSE delta chunks to emit (default: 3)")
    p.add_argument("--sse-delay-ms", type=int, default=int(os.getenv("SSE_DELAY_MS", "50")),
                   help="Delay between SSE chunks in milliseconds (default: 50)")
    p.add_argument("--json-delay-ms", type=int, default=int(os.getenv("JSON_DELAY_MS", "0")),
                   help="Artificial delay before JSON response in milliseconds (default: 0)")
    return p.parse_args(argv)


def bool_env(name: str, default: bool) -> bool:
    v = os.getenv(name)
    if v is None:
        return default
    v = v.strip().lower()
    return v in ("1", "true", "yes", "on")


def run_server(host: str, port: int, **server_opts: Any) -> ThreadingHTTPServer:
    httpd = ThreadingHTTPServer((host, port), MockResponsesHandler)
    # Attach options to server
    for k, v in server_opts.items():
        setattr(httpd, k, v)
    return httpd


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    httpd = run_server(
        args.host,
        args.port,
        require_auth=args.require_auth,
        api_key=args.api_key,
        sse_chunks=args.sse_chunks,
        sse_delay_ms=args.sse_delay_ms,
        json_delay_ms=args.json_delay_ms,
    )
    print(f"Mock OpenAI Responses listening on http://{args.host}:{args.port}")
    print(f"  require_auth={args.require_auth} expected_api_key={'<set>' if args.api_key else '<any>'}")
    print(f"  sse_chunks={args.sse_chunks} sse_delay_ms={args.sse_delay_ms} json_delay_ms={args.json_delay_ms}")

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nShutting down...")
    finally:
        httpd.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
