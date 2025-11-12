#!/usr/bin/env python3
"""
CLI helper that issues Routiium-managed API keys via /keys/generate.

The script loads a local .env (if present) so you can point it at a running
Routiium instance without retyping environment variables every time.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

import requests


def load_env_file(path: Path = Path(".env")) -> None:
    """Populate os.environ with KEY=VALUE pairs from .env if not already set."""
    if not path.is_file():
        return

    for raw_line in path.read_text().splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if not key or key in os.environ:
            continue
        cleaned = value.strip().strip('"').strip("'")
        os.environ[key] = cleaned


def parse_expires_at(value: Optional[str]) -> Optional[int]:
    """Return a unix timestamp from either epoch seconds or ISO-8601."""
    if value is None:
        return None

    stripped = value.strip()
    if not stripped:
        return None
    if stripped.isdigit():
        return int(stripped, 10)

    normalized = stripped.upper().replace("Z", "+00:00")
    try:
        dt = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(
            f"Unable to parse expires-at value '{value}': {exc}"
        ) from exc

    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return int(dt.timestamp())


def env_int(name: str) -> Optional[int]:
    value = os.getenv(name)
    if value is None:
        return None
    stripped = value.strip()
    if not stripped:
        return None
    try:
        return int(stripped, 10)
    except ValueError:
        return None


def build_payload(
    label: str,
    ttl_seconds: Optional[int],
    expires_at: Optional[int],
    scopes: Optional[List[str]],
) -> Dict[str, Any]:
    payload: Dict[str, Any] = {"label": label}
    if expires_at is not None:
        payload["expires_at"] = expires_at
    elif ttl_seconds is not None:
        payload["ttl_seconds"] = ttl_seconds

    if scopes:
        payload["scopes"] = scopes
    return payload


def print_human_readable(data: Dict[str, Any]) -> None:
    token = data.get("token", "")
    expires_at = data.get("expires_at")
    expires_str = "never"
    if isinstance(expires_at, (int, float)):
        expires_str = datetime.fromtimestamp(
            int(expires_at), tz=timezone.utc
        ).isoformat()

    scopes = data.get("scopes") or []
    scopes_text = ", ".join(scopes) if scopes else "default"

    print("✓ API key generated")
    print(f"  id: {data.get('id', 'unknown')}")
    if token:
        print(f"  token: {token}")
    else:
        print("  token: <missing in response>")
    print(f"  label: {data.get('label', 'n/a')}")
    print(f"  scopes: {scopes_text}")
    print(f"  expires_at: {expires_str}")
    if token:
        print("\nStore this token securely; Routiium cannot show it again.")


def main(argv: Optional[List[str]] = None) -> int:
    load_env_file()

    parser = argparse.ArgumentParser(
        description="Generate a Routiium managed API key via /keys/generate."
    )
    parser.add_argument(
        "--base-url",
        default=os.getenv("ROUTIIUM_BASE", "http://127.0.0.1:8088"),
        help="Routiium base URL (no trailing slash).",
    )
    parser.add_argument(
        "--label",
        default=os.getenv("ROUTIIUM_KEY_LABEL", "keygen"),
        help="Label to associate with the generated key.",
    )
    parser.add_argument(
        "--ttl-seconds",
        type=int,
        default=env_int("ROUTIIUM_KEY_TTL"),
        help="TTL in seconds (optional if expires-at is provided).",
    )
    parser.add_argument(
        "--expires-at",
        help="Expiration as unix seconds or ISO-8601 (e.g. 2024-12-31T23:59:59Z).",
    )
    parser.add_argument(
        "--scope",
        dest="scopes",
        action="append",
        help="Add a scope (repeatable).",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=10,
        help="HTTP timeout in seconds (default: 10).",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print the raw JSON response instead of friendly text.",
    )

    args = parser.parse_args(argv)

    ttl_seconds = args.ttl_seconds
    if ttl_seconds is not None and ttl_seconds <= 0:
        parser.error("--ttl-seconds must be a positive integer.")

    expires_at: Optional[int] = None
    if args.expires_at:
        try:
            expires_at = parse_expires_at(args.expires_at)
        except argparse.ArgumentTypeError as exc:
            parser.error(str(exc))

    base_url = args.base_url.rstrip("/")
    payload = build_payload(args.label, ttl_seconds, expires_at, args.scopes)

    try:
        response = requests.post(
            f"{base_url}/keys/generate",
            json=payload,
            timeout=args.timeout,
        )
        response.raise_for_status()
    except requests.RequestException as exc:
        print(f"✗ Failed to call /keys/generate: {exc}", file=sys.stderr)
        return 1

    try:
        data = response.json()
    except ValueError as exc:
        print(f"✗ Routiium returned non-JSON body: {exc}", file=sys.stderr)
        return 1

    if args.json:
        json.dump(data, sys.stdout, indent=2)
        print()
        return 0

    print_human_readable(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
