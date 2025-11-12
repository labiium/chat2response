#!/usr/bin/env bash
#
# Automated end-to-end smoke test for python_tests/chat_cli.py.
# The script optionally builds Routiium, starts the proxy, and feeds
# a canned multi-turn conversation into the chat CLI to ensure the
# Chat→Responses bridge is healthy.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

usage() {
    cat <<'EOF'
Usage: ./run_chat_cli_e2e.sh [options]

Options:
  -m, --message TEXT      User prompt to send to chat_cli (default: friendly smoke prompt)
      --model NAME        Override the chat model passed to chat_cli.py
      --base-url URL      Override ROUTIIUM_BASE for health checks and chat_cli
      --skip-build        Do not run `cargo build --release`
      --reuse-server      Assume a Routiium server is already running; skip build/start/cleanup
      --max-wait SECONDS  Seconds to wait for /status (default: 45)
      --transcript FILE   Copy captured chat_cli output to FILE
      --keep-server-log   Preserve the temporary Routiium log file after exit
  -h, --help              Show this message

Examples:
  ./run_chat_cli_e2e.sh
  ./run_chat_cli_e2e.sh --message "ping" --model gpt-4.1-mini
EOF
}

MESSAGE="Hello from the Routiium chat CLI e2e script!"
MODEL_OVERRIDE=""
BASE_OVERRIDE=""
SKIP_BUILD=0
REUSE_SERVER=0
MAX_WAIT=45
TRANSCRIPT_LOG=""
PRESERVE_LOG=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        -m|--message)
            [[ $# -lt 2 ]] && { log_error "Missing value for $1"; exit 1; }
            MESSAGE="$2"
            shift 2
            ;;
        --model)
            [[ $# -lt 2 ]] && { log_error "Missing value for $1"; exit 1; }
            MODEL_OVERRIDE="$2"
            shift 2
            ;;
        --base-url)
            [[ $# -lt 2 ]] && { log_error "Missing value for $1"; exit 1; }
            BASE_OVERRIDE="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --reuse-server)
            REUSE_SERVER=1
            shift
            ;;
        --max-wait)
            [[ $# -lt 2 ]] && { log_error "Missing value for $1"; exit 1; }
            MAX_WAIT="$2"
            shift 2
            ;;
        --transcript)
            [[ $# -lt 2 ]] && { log_error "Missing value for $1"; exit 1; }
            TRANSCRIPT_LOG="$2"
            shift 2
            ;;
        --keep-server-log)
            PRESERVE_LOG=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON_TESTS_DIR="$PROJECT_ROOT/python_tests"
CHAT_CLI_PATH="$PYTHON_TESTS_DIR/chat_cli.py"
ENV_FILE="$PROJECT_ROOT/.env"
SERVER_BIN="$PROJECT_ROOT/target/release/routiium"

SERVER_PID=""
SERVER_LOG=""
SERVER_STARTED=0
CLI_TMP_LOG=""
VENV_PYTHON=""
EFFECTIVE_ROUTIIUM_BASE=""

cleanup() {
    set +e
    if [[ $SERVER_STARTED -eq 1 && -n "$SERVER_PID" ]]; then
        log_info "Stopping Routiium server (PID: $SERVER_PID)"
        kill "$SERVER_PID" >/dev/null 2>&1 || true
        wait "$SERVER_PID" >/dev/null 2>&1 || true
    fi

    if [[ $PRESERVE_LOG -eq 0 && -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        rm -f "$SERVER_LOG"
    elif [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        log_info "Server log preserved at $SERVER_LOG"
    fi

    if [[ -n "$CLI_TMP_LOG" && -f "$CLI_TMP_LOG" ]]; then
        rm -f "$CLI_TMP_LOG"
    fi
    set -e
}
trap cleanup EXIT INT TERM

abort() {
    log_error "$1"
    if [[ -n "$SERVER_LOG" && -f "$SERVER_LOG" ]]; then
        log_warn "Last 40 lines from Routiium log ($SERVER_LOG):"
        tail -n 40 "$SERVER_LOG" || true
    fi
    if [[ -n "$CLI_TMP_LOG" && -f "$CLI_TMP_LOG" ]]; then
        log_warn "chat_cli transcript captured at $CLI_TMP_LOG"
    fi
    exit 1
}

require_env_file() {
    if [[ ! -f "$ENV_FILE" ]]; then
        abort ".env file not found at $ENV_FILE"
    fi
}

load_env_file() {
    while IFS= read -r line; do
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        if [[ "$line" =~ ^([A-Z_]+)=(.*)$ ]]; then
            key="${BASH_REMATCH[1]}"
            value="${BASH_REMATCH[2]}"
            value="${value%\"}"
            value="${value#\"}"
            value="${value%\'}"
            value="${value#\'}"
            export "$key=$value"
        fi
    done < "$ENV_FILE"

    if [[ -n "$BASE_OVERRIDE" ]]; then
        EFFECTIVE_ROUTIIUM_BASE="$BASE_OVERRIDE"
    elif [[ -n "${ROUTIIUM_BASE:-}" ]]; then
        EFFECTIVE_ROUTIIUM_BASE="$ROUTIIUM_BASE"
    else
        EFFECTIVE_ROUTIIUM_BASE="http://127.0.0.1:8099"
    fi
    EFFECTIVE_ROUTIIUM_BASE="${EFFECTIVE_ROUTIIUM_BASE%/}"
    export ROUTIIUM_BASE="$EFFECTIVE_ROUTIIUM_BASE"
}

ensure_uv() {
    if command -v uv >/dev/null 2>&1; then
        log_info "uv found ($(uv --version))"
        return
    fi

    log_info "Installing uv..."
    if ! command -v curl >/dev/null 2>&1; then
        abort "curl is required to install uv automatically"
    fi
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.cargo/bin:$PATH"

    if ! command -v uv >/dev/null 2>&1; then
        abort "uv installation failed; please install it manually"
    fi
}

ensure_python_env() {
    ensure_uv

    if [[ ! -d "$PYTHON_TESTS_DIR/.venv" ]]; then
        log_info "Creating Python virtual environment with uv..."
        (cd "$PYTHON_TESTS_DIR" && uv venv)
    else
        log_info "Reusing Python virtual environment at $PYTHON_TESTS_DIR/.venv"
    fi

    # shellcheck disable=SC1091
    source "$PYTHON_TESTS_DIR/.venv/bin/activate"
    log_info "Installing chat_cli dependencies (idempotent)..."
    (cd "$PYTHON_TESTS_DIR" && uv pip install -e .)
    VENV_PYTHON="$PYTHON_TESTS_DIR/.venv/bin/python"
}

maybe_build_routiium() {
    if [[ $REUSE_SERVER -eq 1 ]]; then
        log_info "Reusing existing Routiium server; skipping build."
        return
    fi

    if [[ $SKIP_BUILD -eq 1 ]]; then
        log_info "Skipping Routiium build as requested."
        return
    fi

    if ! command -v cargo >/dev/null 2>&1; then
        abort "cargo is required to build Routiium"
    fi

    log_info "Building Routiium release binary..."
    (cd "$PROJECT_ROOT" && cargo build --release)
}

wait_for_server() {
    local waited=0
    while [[ $waited -lt $MAX_WAIT ]]; do
        if curl -sSf "$EFFECTIVE_ROUTIIUM_BASE/status" >/dev/null 2>&1; then
            log_success "Routiium is responding at $EFFECTIVE_ROUTIIUM_BASE"
            return
        fi
        if [[ $SERVER_STARTED -eq 1 && -n "$SERVER_PID" ]] && ! ps -p "$SERVER_PID" >/dev/null 2>&1; then
            abort "Routiium server exited unexpectedly; see $SERVER_LOG"
        fi
        sleep 1
        waited=$((waited + 1))
    done
    abort "Server did not become ready within ${MAX_WAIT}s (checked $EFFECTIVE_ROUTIIUM_BASE/status)"
}

start_routiium_server() {
    if [[ $REUSE_SERVER -eq 1 ]]; then
        log_info "Reusing Routiium server at $EFFECTIVE_ROUTIIUM_BASE"
        if ! curl -sSf "$EFFECTIVE_ROUTIIUM_BASE/status" >/dev/null 2>&1; then
            abort "ROUTIIUM_BASE=$EFFECTIVE_ROUTIIUM_BASE is not responding; remove --reuse-server to auto-start"
        fi
        return
    fi

    if [[ ! -x "$SERVER_BIN" ]]; then
        abort "Routiium binary not found at $SERVER_BIN; run cargo build --release first or drop --skip-build"
    fi

    SERVER_LOG="$(mktemp /tmp/routiium_cli_e2e_server_XXXX.log)"

    local cli_args=()
    if [[ -f "$PYTHON_TESTS_DIR/router_aliases.json" ]]; then
        cli_args+=("--router-config=$PYTHON_TESTS_DIR/router_aliases.json")
    fi
    if [[ -f "$PYTHON_TESTS_DIR/mcp/mcp.json" ]]; then
        cli_args+=("--mcp-config=$PYTHON_TESTS_DIR/mcp/mcp.json")
    fi
    if [[ -f "$PYTHON_TESTS_DIR/system_prompt.json" ]]; then
        cli_args+=("--system-prompt-config=$PYTHON_TESTS_DIR/system_prompt.json")
    fi

    log_info "Starting Routiium server (logs: $SERVER_LOG)"
    (cd "$PROJECT_ROOT" && "$SERVER_BIN" "${cli_args[@]}") >"$SERVER_LOG" 2>&1 &
    SERVER_PID=$!
    SERVER_STARTED=1

    wait_for_server
}

run_chat_cli() {
    if [[ -z "$VENV_PYTHON" || ! -x "$VENV_PYTHON" ]]; then
        abort "Python environment not initialized; cannot invoke chat_cli.py"
    fi
    if [[ ! -f "$CHAT_CLI_PATH" ]]; then
        abort "chat_cli.py not found at $CHAT_CLI_PATH"
    fi

    local model_args=()
    if [[ -n "$MODEL_OVERRIDE" ]]; then
        model_args=(--model "$MODEL_OVERRIDE")
    fi

    CLI_TMP_LOG="$(mktemp /tmp/routiium_chat_cli_e2e_XXXX.log)"

    log_info "Running chat_cli.py smoke conversation..."
    pushd "$PROJECT_ROOT" >/dev/null
    printf "%s\n/exit\n" "$MESSAGE" | "$VENV_PYTHON" "$CHAT_CLI_PATH" "${model_args[@]}" | tee "$CLI_TMP_LOG"
    local printf_status=${PIPESTATUS[0]}
    local python_status=${PIPESTATUS[1]}
    local tee_status=${PIPESTATUS[2]}
    popd >/dev/null

    if [[ $python_status -ne 0 ]]; then
        abort "chat_cli.py exited with status $python_status (printf=$printf_status, tee=$tee_status)"
    fi

    if [[ -n "$TRANSCRIPT_LOG" ]]; then
        mkdir -p "$(dirname "$TRANSCRIPT_LOG")"
        cp "$CLI_TMP_LOG" "$TRANSCRIPT_LOG"
        log_info "Copied transcript to $TRANSCRIPT_LOG"
    fi

    if ! grep -q "^Assistant:" "$CLI_TMP_LOG"; then
        abort "Assistant response was not observed; see $CLI_TMP_LOG"
    fi
    if grep -q "\[no assistant text\]" "$CLI_TMP_LOG"; then
        abort "Assistant response was empty; see $CLI_TMP_LOG"
    fi

    log_success "chat_cli.py successfully completed an end-to-end round-trip."
}

main() {
    require_env_file
    load_env_file
    ensure_python_env
    maybe_build_routiium
    start_routiium_server
    run_chat_cli
}

main "$@"
