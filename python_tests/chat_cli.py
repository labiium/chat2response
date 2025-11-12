#!/usr/bin/env python3
"""
Tiny multi-turn Responses CLI for Routiium.

The script keeps a Chat-style transcript, ensures a managed API key exists
by calling Routiium's `/keys/generate`, and then issues streaming requests to
`ROUTIIUM_BASE/v1/responses` through the official OpenAI Python SDK.

Environment (loads `.env` automatically if present):
    ROUTIIUM_BASE          – Routiium root URL (default http://127.0.0.1:8088)
    ROUTIIUM_ACCESS_TOKEN  – Reuse an existing key instead of generating a new one
    ROUTIIUM_KEY_TTL       – TTL (seconds) for generated keys (default 3600)
    MODEL or CHAT_MODEL    – Default chat model/alias

CLI controls:
    /reset  -> clears the local transcript (new conversation)
    /exit   -> quits the program (aliases: /quit, :q, ctrl+d)
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Tuple, Union

import requests
from openai import OpenAI, OpenAIError


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


def parse_float(value: Optional[str]) -> Optional[float]:
    if value is None or value.strip() == "":
        return None
    try:
        return float(value)
    except ValueError:
        return None


def parse_int(value: Optional[str]) -> Optional[int]:
    if value is None or value.strip() == "":
        return None
    try:
        return int(value)
    except ValueError:
        return None


def ensure_api_key(
    base_url: str,
    existing_token: Optional[str],
    label: str,
    ttl_seconds: Optional[int],
) -> str:
    """Return a Routiium API key, generating one if needed."""
    if existing_token:
        return existing_token

    payload: Dict[str, Union[str, int]] = {"label": label}
    if ttl_seconds:
        payload["ttl_seconds"] = ttl_seconds

    try:
        response = requests.post(
            f"{base_url}/keys/generate",
            json=payload,
            timeout=10,
        )
        response.raise_for_status()
    except requests.RequestException as exc:
        raise RuntimeError(f"Failed to generate Routiium API key: {exc}") from exc

    data = response.json()
    token = data.get("token")
    if not isinstance(token, str) or not token:
        raise RuntimeError(f"Key generation succeeded but token missing: {data}")

    print(f"Generated Routiium key: {token[:12]}… (label={label})")
    return token


def _format_content(
    content: Union[str, Sequence[object], None],
) -> Union[str, List[Dict[str, object]]]:
    """Normalize transcript content into Responses API-friendly content."""
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, Sequence):
        parts: List[Dict[str, object]] = []
        for item in content:
            if isinstance(item, str):
                stripped = item.strip()
                if stripped:
                    parts.append({"type": "input_text", "text": stripped})
            elif isinstance(item, dict):
                part_type = item.get("type")
                if part_type == "text":
                    text = item.get("text")
                    if isinstance(text, str) and text.strip():
                        parts.append({"type": "input_text", "text": text.strip()})
                elif part_type == "image_url":
                    new_part: Dict[str, object] = {"type": "input_image"}
                    image_url = item.get("image_url")
                    if isinstance(image_url, dict):
                        url = image_url.get("url")
                        if isinstance(url, str):
                            new_part["image_url"] = url
                        detail = image_url.get("detail")
                        if isinstance(detail, str):
                            new_part["detail"] = detail
                    elif isinstance(image_url, str):
                        new_part["image_url"] = image_url
                    url = item.get("url")
                    if isinstance(url, str) and "image_url" not in new_part:
                        new_part["image_url"] = url
                    if new_part.get("image_url"):
                        parts.append(new_part)
                else:
                    parts.append(item)
        return parts
    return str(content)


def transcript_to_responses_input(
    transcript: List[Dict[str, str]],
) -> List[Dict[str, object]]:
    """Convert chat transcript into Responses API `input` payload."""
    formatted: List[Dict[str, object]] = []
    for message in transcript:
        role = message.get("role", "user")
        content = message.get("content")
        parts = _format_content(content)
        formatted.append({"role": role, "content": parts})
    return formatted


def transcript_to_chat_messages(
    transcript: List[Dict[str, str]],
) -> List[Dict[str, object]]:
    """Convert the transcript into Chat Completions-compatible messages."""
    messages: List[Dict[str, object]] = []
    for message in transcript:
        role = message.get("role", "user")
        content = message.get("content", "")
        messages.append({"role": role, "content": content or ""})
    return messages


def build_responses_request_kwargs(
    model: str,
    transcript: List[Dict[str, str]],
    temperature: Optional[float],
    top_p: Optional[float],
    max_tokens: Optional[int],
    response_format: Optional[str],
    conversation_id: Optional[str],
) -> Dict[str, object]:
    """Build kwargs for OpenAI Responses call."""
    kwargs: Dict[str, object] = {
        "model": model,
        "input": transcript_to_responses_input(transcript),
    }
    if temperature is not None:
        kwargs["temperature"] = temperature
    if top_p is not None:
        kwargs["top_p"] = top_p
    if max_tokens is not None:
        kwargs["max_output_tokens"] = max_tokens
    if response_format:
        kwargs["response_format"] = {"type": response_format}
    if conversation_id:
        kwargs["conversation"] = conversation_id
    return kwargs


def build_chat_request_kwargs(
    model: str,
    transcript: List[Dict[str, str]],
    temperature: Optional[float],
    top_p: Optional[float],
    max_tokens: Optional[int],
    response_format: Optional[str],
) -> Dict[str, object]:
    """Build kwargs for OpenAI Chat Completions call."""
    kwargs: Dict[str, object] = {
        "model": model,
        "messages": transcript_to_chat_messages(transcript),
    }
    if temperature is not None:
        kwargs["temperature"] = temperature
    if top_p is not None:
        kwargs["top_p"] = top_p
    if max_tokens is not None:
        kwargs["max_tokens"] = max_tokens
    if response_format:
        kwargs["response_format"] = {"type": response_format}
    return kwargs


def _normalize_text_chunks(value: Union[str, Sequence[object], None]) -> Optional[str]:
    if isinstance(value, str):
        stripped = value.strip()
        return stripped or None
    if isinstance(value, Sequence):
        collected: List[str] = []
        for item in value:
            if isinstance(item, dict):
                text = item.get("text")
                if isinstance(text, dict):
                    text = text.get("value")
                if isinstance(text, str) and text.strip():
                    collected.append(text.strip())
            elif isinstance(item, str) and item.strip():
                collected.append(item.strip())
        if collected:
            return "\n".join(collected)
    return None


def extract_responses_text(response: object) -> Optional[str]:
    """Extract assistant text from a Responses API response."""
    output_items = _get_field(response, "output") or []
    collected: List[str] = []
    for item in output_items:
        if isinstance(item, dict):
            if item.get("type") == "message":
                content = item.get("content")
                text = _normalize_text_chunks(content)
                if text:
                    collected.append(text)
            elif item.get("type") in {"assistant_message", "assistant"}:
                content = item.get("content")
                if isinstance(content, str) and content.strip():
                    collected.append(content.strip())
        else:
            message = _get_field(item, "message")
            content = _get_field(message, "content")
            text = _normalize_text_chunks(content)
            if text:
                collected.append(text)
    if collected:
        return "\n\n".join(collected)
    output_text = _get_field(response, "output_text")
    if isinstance(output_text, str) and output_text.strip():
        return output_text.strip()
    return None


def _get_field(obj: object, key: str) -> Any:
    if isinstance(obj, dict):
        return obj.get(key)
    return getattr(obj, key, None)


def stream_with_openai(client: OpenAI, response_kwargs: Dict[str, object]) -> str:
    """Stream Responses API tokens using the official SDK."""
    collected: List[str] = []
    printed = False
    stream = client.responses.create(stream=True, **response_kwargs)
    for chunk in stream:
        if hasattr(chunk, "type") and getattr(chunk, "type") == "response.error":
            error = getattr(chunk, "error", None)
            raise RuntimeError(f"Responses stream error: {error}")
        delta = getattr(chunk, "output_text_delta", None)
        if delta:
            print(delta, end="", flush=True)
            collected.append(delta)
            printed = True
    if printed:
        print()
    final_text = "".join(collected).strip()
    if not final_text:
        final_text = "[no assistant text]"
        if not printed:
            print(final_text)
    return final_text


def _extract_chat_delta_text(delta_content: Any) -> Optional[str]:
    if delta_content is None:
        return None
    if isinstance(delta_content, str):
        return delta_content
    if isinstance(delta_content, Sequence):
        pieces: List[str] = []
        for item in delta_content:
            if isinstance(item, str):
                pieces.append(item)
            elif isinstance(item, dict):
                text = item.get("text")
                if isinstance(text, dict):
                    value = text.get("value")
                    if isinstance(value, str):
                        pieces.append(value)
                elif isinstance(text, str):
                    pieces.append(text)
        if pieces:
            return "".join(pieces)
    return None


def stream_chat_completion(
    client: OpenAI,
    chat_kwargs: Dict[str, object],
    extra_query: Optional[Dict[str, str]],
) -> Tuple[str, Optional[str]]:
    """Stream Chat Completions tokens via Routiium."""
    collected: List[str] = []
    printed = False
    last_response_id: Optional[str] = None
    stream = client.chat.completions.create(
        stream=True,
        extra_query=extra_query,
        **chat_kwargs,
    )
    for chunk in stream:
        if hasattr(chunk, "id") and isinstance(chunk.id, str):
            last_response_id = chunk.id
        for choice in getattr(chunk, "choices", []) or []:
            delta = getattr(choice, "delta", None)
            if not delta:
                continue
            text = _extract_chat_delta_text(getattr(delta, "content", None))
            if text:
                print(text, end="", flush=True)
                collected.append(text)
                printed = True
    if printed:
        print()
    final_text = "".join(collected).strip()
    if not final_text:
        final_text = "[no assistant text]"
        if not printed:
            print(final_text)
    return final_text, last_response_id


def print_banner(model: str, base_url: str, api_key: str, mode: str) -> None:
    endpoint = "/v1/chat/completions" if mode == "chat" else "/v1/responses"
    print(f"Routiium base: {base_url}")
    print(f"Gateway endpoint: {base_url}{endpoint}")
    print(f"Model: {model}")
    print(f"API key: {api_key[:8]}… (managed)")
    print(f"Mode: {mode}")
    print("Commands: /reset, /exit")
    print("-" * 50)


def main() -> int:
    load_env_file()

    env_temp = parse_float(os.getenv("CHAT_TEMPERATURE"))
    env_top_p = parse_float(os.getenv("CHAT_TOP_P"))
    env_max_tokens = parse_int(os.getenv("CHAT_MAX_TOKENS"))

    parser = argparse.ArgumentParser(
        description="Interactive multi-turn tester for Routiium's streaming Responses endpoint."
    )
    parser.add_argument(
        "--routiium-base",
        default=os.getenv("ROUTIIUM_BASE", "http://127.0.0.1:8088"),
        help="Base URL of the Routiium proxy (no /v1).",
    )
    env_model = os.getenv("CHAT_MODEL") or os.getenv("MODEL")
    banned_models = {"gpt-4o", "gpt-4o-mini", "gpt-4o-mini-2024-08-06"}
    model_default = env_model or "gpt-4.1-nano"
    if model_default.lower() in banned_models:
        print(
            f"[info] Model '{model_default}' is disallowed; using gpt-4.1-nano instead.",
            file=sys.stderr,
        )
        model_default = "gpt-4.1-nano"

    parser.add_argument(
        "--model",
        default=model_default,
        help="Chat model alias to request.",
    )
    parser.add_argument(
        "--system",
        default=os.getenv("SYSTEM_PROMPT"),
        help="Optional system prompt inserted at the top of the conversation.",
    )
    parser.add_argument(
        "--api-key",
        default=os.getenv("ROUTIIUM_ACCESS_TOKEN"),
        help="Existing Routiium managed key; if omitted the script generates one.",
    )
    parser.add_argument(
        "--key-label",
        default=os.getenv("ROUTIIUM_KEY_LABEL", "cli-session"),
        help="Label to use when generating keys.",
    )
    parser.add_argument(
        "--key-ttl",
        type=int,
        default=parse_int(os.getenv("ROUTIIUM_KEY_TTL")) or 3600,
        help="TTL (seconds) for generated keys (default 3600).",
    )
    parser.add_argument(
        "--conversation-id",
        default=os.getenv("CHAT_CONVERSATION_ID"),
        help="Optional client-side conversation id (not persisted in Routiium).",
    )
    parser.add_argument(
        "--organization",
        default=os.getenv("ROUTIIUM_ORG"),
        help="Optional organization header forwarded to Routiium.",
    )
    parser.add_argument(
        "--project",
        default=os.getenv("ROUTIIUM_PROJECT"),
        help="Optional project header forwarded to Routiium.",
    )
    parser.add_argument(
        "--temperature",
        type=float,
        default=env_temp,
        help="Optional sampling temperature.",
    )
    parser.add_argument(
        "--top-p",
        type=float,
        default=env_top_p,
        help="Optional nucleus sampling top_p value.",
    )
    parser.add_argument(
        "--max-tokens",
        type=int,
        default=env_max_tokens,
        help="Optional max output tokens cap.",
    )
    parser.add_argument(
        "--response-format",
        default=os.getenv("CHAT_RESPONSE_FORMAT"),
        help="Optional response_format.type (e.g., json_object).",
    )
    parser.add_argument(
        "--mode",
        choices=("chat", "responses"),
        default=os.getenv("CHAT_MODE", "chat"),
        help="Which Routiium endpoint to exercise.",
    )
    args = parser.parse_args()

    routiium_base = args.routiium_base.rstrip("/")

    try:
        api_key = ensure_api_key(
            routiium_base,
            args.api_key,
            args.key_label,
            args.key_ttl,
        )
    except RuntimeError as exc:
        print(exc, file=sys.stderr)
        return 1

    conversation_id = args.conversation_id
    previous_response_id: Optional[str] = None
    transcript: List[Dict[str, str]] = []
    if args.system:
        transcript.append({"role": "system", "content": args.system})

    print_banner(args.model, routiium_base, api_key, args.mode)

    client_kwargs = {
        "base_url": f"{routiium_base}/v1",
        "api_key": api_key,
    }
    if args.organization:
        client_kwargs["organization"] = args.organization
    if args.project:
        client_kwargs["project"] = args.project

    client = OpenAI(**client_kwargs)

    while True:
        try:
            user_input = input("You: ").strip()
        except (EOFError, KeyboardInterrupt):
            print("\nExiting.")
            break

        if not user_input:
            continue
        if user_input.lower() in {"/exit", "/quit", ":q"}:
            break
        if user_input.lower() == "/reset":
            transcript = []
            if args.system:
                transcript.append({"role": "system", "content": args.system})
            print("Conversation reset.\n")
            continue

        user_message = {"role": "user", "content": user_input}
        transcript.append(user_message)

        assistant_text: Optional[str] = None
        print("Assistant: ", end="", flush=True)
        try:
            if args.mode == "chat":
                chat_kwargs = build_chat_request_kwargs(
                    args.model,
                    transcript,
                    args.temperature,
                    args.top_p,
                    args.max_tokens,
                    args.response_format,
                )
                extra_query: Dict[str, str] = {}
                if conversation_id:
                    extra_query["conversation_id"] = conversation_id
                if previous_response_id:
                    extra_query["previous_response_id"] = previous_response_id
                assistant_text, latest_response_id = stream_chat_completion(
                    client,
                    chat_kwargs,
                    extra_query or None,
                )
                if latest_response_id:
                    previous_response_id = latest_response_id
            else:
                response_kwargs = build_responses_request_kwargs(
                    args.model,
                    transcript,
                    args.temperature,
                    args.top_p,
                    args.max_tokens,
                    args.response_format,
                    conversation_id,
                )
                assistant_text = stream_with_openai(client, response_kwargs)
        except OpenAIError as exc:
            print()
            transcript.pop()
            print(f"[Routiium error] {exc}", file=sys.stderr)
            continue
        except Exception as exc:  # pylint: disable=broad-except
            print()
            transcript.pop()
            print(f"[error] {exc}", file=sys.stderr)
            continue

        if not assistant_text:
            assistant_text = "[no assistant text]"
        print()
        transcript.append({"role": "assistant", "content": assistant_text})

    return 0


if __name__ == "__main__":
    sys.exit(main())
