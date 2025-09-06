#!/usr/bin/env python3
"""
run_official_responses_via_chat2response.py

A small, self-contained script to exercise:
1) Official OpenAI Responses API (direct) using the OpenAI Python SDK.
2) Chat2Response service:
   - POST /convert to inspect the translated Responses payload
   - POST /proxy to forward the Chat-style request to the Responses API and return native Responses output

Requirements
- Python 3.9+
- pip install openai requests

Usage examples
1) Direct Responses (official API):
   OPENAI_API_KEY=sk-... python chat2response/e2e/run_official_responses_via_chat2response.py --mode direct --model gpt-4o-mini --prompt "Say hi"

2) Convert-only via Chat2Response (no network to OpenAI):
   python chat2response/e2e/run_official_responses_via_chat2response.py --mode convert --chat2response-base http://127.0.0.1:8088 --model gpt-4o-mini --prompt "Say hi"

3) Proxy via Chat2Response (forwards to Responses):
   # Ensure your Chat2Response server is running
   # and environment variables: OPENAI_API_KEY=sk-..., optionally OPENAI_BASE_URL=https://api.openai.com/v1
   python chat2response/e2e/run_official_responses_via_chat2response.py --mode proxy --chat2response-base http://127.0.0.1:8088 --model gpt-4o-mini --prompt "Say hi"

Streaming notes
- Direct Responses streaming uses the OpenAI client’s streaming API when available.
- Proxy streaming sets "stream": true; this script will display the raw SSE returned by Chat2Response.

Exit codes
- 0 on success; non-zero on failures.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any, Dict, Optional

import requests

# The OpenAI client is optional for convert/proxy, but required for --mode direct
try:
    from openai import OpenAI  # type: ignore
except Exception:
    OpenAI = None  # type: ignore


def _eprint(msg: str) -> None:
    sys.stderr.write(msg + "\n")
    sys.stderr.flush()


def _load_env_file(path: Optional[str]) -> None:
    """
    Lightweight .env loader. If `path` is provided and exists, load it.
    If `path` is None and a local ".env" exists, load that.
    Only sets keys that are not already present in os.environ.
    """
    target = path or ".env"
    try:
        if not os.path.isfile(target):
            return
        with open(target, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line or line.startswith("#"):
                    continue
                if "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k = k.strip()
                v = v.strip().strip('"').strip("'")
                if k and k not in os.environ:
                    os.environ[k] = v
    except Exception as e:
        _eprint(f"Warning: could not load env file {target}: {e}")


def _apply_env_overrides(args: argparse.Namespace) -> argparse.Namespace:
    """
    Apply environment-based overrides to CLI args when the user did not
    explicitly set a value (i.e., args still at defaults).
    - CHAT2RESPONSE_BASE -> chat2response_base (default http://127.0.0.1:8088)
    - OPENAI_BASE_URL -> openai_base (default None)
    - MODEL or OPENAI_MODEL -> model (default gpt-4o-mini)
    - PROMPT -> prompt (default 'Say hi')
    - CONVERSATION_ID -> conversation_id (default None)
    - STREAM -> stream (default False) with truthy parsing
    """
    # Bases
    c2r = os.getenv("CHAT2RESPONSE_BASE")
    if c2r and args.chat2response_base == "http://127.0.0.1:8088":
        args.chat2response_base = c2r

    oai_base = os.getenv("OPENAI_BASE_URL")
    if oai_base and (args.openai_base is None or args.openai_base == ""):
        args.openai_base = oai_base

    # Model/prompt
    model_env = os.getenv("MODEL") or os.getenv("OPENAI_MODEL")
    if model_env and args.model == "gpt-4o-mini":
        args.model = model_env

    prompt_env = os.getenv("PROMPT")
    if prompt_env and args.prompt == "Say hi":
        args.prompt = prompt_env

    # Conversation/state
    conv = os.getenv("CONVERSATION_ID")
    if conv and (args.conversation_id is None or args.conversation_id == ""):
        args.conversation_id = conv

    stream_env = os.getenv("STREAM", "")
    if not args.stream and stream_env.strip().lower() in ("1", "true", "yes", "on"):
        args.stream = True

    return args


def call_openai_responses_direct(
    model: str,
    prompt: str,
    stream: bool,
    openai_base: Optional[str],
    timeout: float = 60.0,
) -> int:
    if OpenAI is None:
        _eprint("OpenAI SDK not installed. Please `pip install openai`.")
        return 2

    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        _eprint("OPENAI_API_KEY environment variable is required for direct mode.")
        return 2

    client_kwargs: Dict[str, Any] = {"api_key": api_key}
    if openai_base:
        client_kwargs["base_url"] = openai_base

    client = OpenAI(**client_kwargs)  # type: ignore

    if stream:
        # Prefer streaming if SDK supports it.
        if hasattr(client.responses, "stream"):
            _eprint("Direct: streaming via OpenAI Responses API...")
            try:
                # Newer SDKs support a context manager for streaming
                with client.responses.stream(
                    model=model,
                    input=prompt,
                ) as s:
                    for event in s:
                        # Print raw-ish event for visibility
                        print(json.dumps({"type": getattr(event, "type", "event"), "data": str(event)}))
                    final = s.get_final_response()
                    # Convert to dict robustly
                    data = _model_to_dict(final)
                    print(json.dumps({"final": data}, indent=2))
                return 0
            except Exception as e:
                _eprint(f"Direct stream error: {e}")
                return 1
        else:
            _eprint("OpenAI client.responses.stream not available in this SDK; falling back to non-streaming.")
            # fallthrough to non-stream
            stream = False

    # Non-streaming
    _eprint("Direct: non-streaming via OpenAI Responses API...")
    try:
        # Prefer with_raw_response for cross-version stability
        if hasattr(client.responses, "with_raw_response"):
            resp = client.responses.with_raw_response.create(model=model, input=prompt)
            # LegacyAPIResponse across versions; try parse() first
            if hasattr(resp, "parse"):
                obj = resp.parse()
                data = _model_to_dict(obj)
            else:
                # fallback read + json
                raw = resp.read()
                text = raw.decode("utf-8") if isinstance(raw, (bytes, bytearray)) else str(raw)
                data = json.loads(text)
        else:
            obj = client.responses.create(model=model, input=prompt)
            data = _model_to_dict(obj)

        print(json.dumps(data, indent=2))
        return 0
    except Exception as e:
        _eprint(f"Direct request error: {e}")
        return 1


def call_chat2response_convert(
    base: str,
    model: str,
    prompt: str,
    conversation_id: Optional[str],
    timeout: float = 30.0,
) -> int:
    url = f"{base.rstrip('/')}/convert"
    params = {}
    if conversation_id:
        params["conversation_id"] = conversation_id

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
    }

    _eprint(f"POST {url} ...")
    try:
        r = requests.post(url, params=params, json=payload, timeout=timeout)
        r.raise_for_status()
    except Exception as e:
        _eprint(f"/convert error: {e}")
        if hasattr(e, "response") and getattr(e, "response") is not None:
            _eprint(f"Response body: {getattr(e, 'response').text}")
        return 1

    try:
        data = r.json()
    except Exception:
        data = r.text
    print(json.dumps({"converted": data}, indent=2))
    return 0


def call_chat2response_proxy(
    base: str,
    model: str,
    prompt: str,
    conversation_id: Optional[str],
    stream: bool,
    timeout: float = 120.0,
) -> int:
    url = f"{base.rstrip('/')}/proxy"
    params = {}
    if conversation_id:
        params["conversation_id"] = conversation_id

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": bool(stream),
    }

    _eprint(f"POST {url} (stream={stream}) ...")
    try:
        # If stream=True, we still rely on server semantics:
        # - The current Chat2Response buffers SSE and returns the entire SSE body at once.
        # We'll request a streaming response from requests and print lines as they arrive.
        r = requests.post(url, params=params, json=payload, timeout=timeout, stream=stream)
        r.raise_for_status()
    except Exception as e:
        _eprint(f"/proxy error: {e}")
        if hasattr(e, "response") and getattr(e, "response") is not None:
            _eprint(f"Response body: {getattr(e, 'response').text}")
        return 1

    if stream:
        # Attempt to read as SSE lines
        _eprint("Streaming response (SSE/raw):")
        try:
            for line in r.iter_lines(decode_unicode=True):
                if line is None:
                    continue
                print(line)
        except Exception as e:
            _eprint(f"Error while reading stream: {e}")
            return 1
        return 0

    # Non-streaming JSON
    try:
        data = r.json()
    except Exception:
        data = r.text
    print(json.dumps({"proxied": data}, indent=2))
    return 0


def _model_to_dict(obj: Any) -> Dict[str, Any]:
    """
    Best-effort conversion of OpenAI SDK response objects to dict, across versions.
    """
    if obj is None:
        return {}
    for attr in ("model_dump", "to_dict", "dict"):
        if hasattr(obj, attr):
            try:
                fn = getattr(obj, attr)
                d = fn()  # type: ignore
                if isinstance(d, dict):
                    return d
            except Exception:
                pass
    if hasattr(obj, "model_dump_json"):
        try:
            return json.loads(obj.model_dump_json())
        except Exception:
            pass
    # Fallback: brute force
    try:
        return json.loads(json.dumps(obj, default=lambda o: getattr(o, "__dict__", str(o))))
    except Exception:
        return {"raw": str(obj)}


def parse_args(argv: list[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Exercise OpenAI Responses API and Chat2Response convert/proxy flows.")
    p.add_argument("--mode", choices=["direct", "convert", "proxy"], required=True, help="Which path to exercise.")
    p.add_argument("--chat2response-base", default="http://127.0.0.1:8088", help="Base URL for Chat2Response service. Can also be set via CHAT2RESPONSE_BASE env or .env.")
    p.add_argument("--openai-base", default=None, help="Optional base URL override for OpenAI SDK (e.g., https://api.openai.com/v1). Can also be set via OPENAI_BASE_URL env or .env.")
    p.add_argument("--model", default="gpt-4o-mini", help="Model name. Can also be set via MODEL or OPENAI_MODEL env.")
    p.add_argument("--prompt", default="Say hi", help="User prompt text. Can also be set via PROMPT env.")
    p.add_argument("--conversation-id", default=None, help="Optional Responses conversation id for stateful flows. Can also be set via CONVERSATION_ID env.")
    p.add_argument("--stream", action="store_true", help="Enable streaming (when supported). Can also be enabled via STREAM=true in env.")
    p.add_argument("--env-file", default=None, help="Path to a .env file to load (defaults to ./.env if present).")
    return p.parse_args(argv)


def main(argv: list[str]) -> int:
    # Parse args (to get --env-file), then load .env, then apply environment overrides if args are defaults.
    args = parse_args(argv)
    _load_env_file(args.env_file)
    # If no --env-file, still try a local .env
    if args.env_file is None and os.path.isfile(".env"):
        _load_env_file(".env")
    args = _apply_env_overrides(args)

    if args.mode == "direct":
        return call_openai_responses_direct(
            model=args.model,
            prompt=args.prompt,
            stream=args.stream,
            openai_base=args.openai_base,
        )

    if args.mode == "convert":
        return call_chat2response_convert(
            base=args.chat2response_base,
            model=args.model,
            prompt=args.prompt,
            conversation_id=args.conversation_id,
        )

    if args.mode == "proxy":
        return call_chat2response_proxy(
            base=args.chat2response_base,
            model=args.model,
            prompt=args.prompt,
            conversation_id=args.conversation_id,
            stream=args.stream,
        )

    _eprint(f"Unknown mode: {args.mode}")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
