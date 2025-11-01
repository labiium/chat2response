#!/usr/bin/env bash
#####################################################################
# QUICK TEST RUNNER FOR CHAT2RESPONSE
#####################################################################
#
# This script runs tests against an already-running chat2response server.
# Use this when you want to run tests without starting/stopping the server.
#
# Usage:
#   ./run_tests.sh              # Run all tests
#   ./run_tests.sh -v           # Verbose output
#   ./run_tests.sh -k test_name # Run specific test
#
#####################################################################

set -e  # Exit on error
set -u  # Exit on undefined variable

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENV_FILE="$PROJECT_ROOT/.env"

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if .env file exists
if [ ! -f "$ENV_FILE" ]; then
    log_error ".env file not found at $ENV_FILE"
    exit 1
fi

# Check if server is running
CHAT2RESPONSE_BASE=$(grep CHAT2RESPONSE_BASE "$ENV_FILE" | cut -d= -f2)
if [ -z "$CHAT2RESPONSE_BASE" ]; then
    CHAT2RESPONSE_BASE="http://127.0.0.1:8099"
fi

log_info "Checking if chat2response server is running at $CHAT2RESPONSE_BASE..."
if ! curl -s "${CHAT2RESPONSE_BASE}/status" > /dev/null 2>&1; then
    log_error "chat2response server is not running at $CHAT2RESPONSE_BASE"
    log_info "Start the server first with: cd .. && cargo run --release"
    exit 1
fi

log_success "Server is running"

# Setup virtual environment if needed
cd "$SCRIPT_DIR"

if [ ! -d ".venv" ]; then
    log_info "Creating virtual environment..."
    uv venv
    source .venv/bin/activate
    log_info "Installing dependencies..."
    uv pip install -e .
else
    source .venv/bin/activate
fi

# Load environment variables
export $(grep -v '^#' "$ENV_FILE" | xargs)

# Run pytest with passed arguments
log_info "Running tests..."
echo ""

if pytest tests/ "$@"; then
    echo ""
    log_success "====================================="
    log_success "All tests passed!"
    log_success "====================================="
    exit 0
else
    echo ""
    log_error "====================================="
    log_error "Some tests failed"
    log_error "====================================="
    exit 1
fi
