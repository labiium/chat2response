#!/usr/bin/env bash
#####################################################################
# SETUP AND TEST SCRIPT FOR CHAT2RESPONSE PYTHON INTEGRATION TESTS
#####################################################################
#
# This script:
# 1. Installs uv if not present
# 2. Sets up Python virtual environment using uv
# 3. Installs dependencies
# 4. Starts chat2response server in background
# 5. Runs integration tests
# 6. Cleans up background processes
#
# Time complexity: O(n) where n is number of dependencies
# Space complexity: O(1) - minimal memory overhead
#
#####################################################################

set -e  # Exit on error
set -u  # Exit on undefined variable

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON_TESTS_DIR="$SCRIPT_DIR"

# Environment file
ENV_FILE="$PROJECT_ROOT/.env"

# Server PID file
SERVER_PID_FILE="/tmp/chat2response_test_server.pid"

#####################################################################
# UTILITY FUNCTIONS
#####################################################################

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

cleanup() {
    log_info "Cleaning up..."

    # Kill chat2response server if running
    if [ -f "$SERVER_PID_FILE" ]; then
        SERVER_PID=$(cat "$SERVER_PID_FILE")
        if ps -p "$SERVER_PID" > /dev/null 2>&1; then
            log_info "Stopping chat2response server (PID: $SERVER_PID)"
            kill "$SERVER_PID" 2>/dev/null || true
            sleep 2
            # Force kill if still running
            if ps -p "$SERVER_PID" > /dev/null 2>&1; then
                kill -9 "$SERVER_PID" 2>/dev/null || true
            fi
        fi
        rm -f "$SERVER_PID_FILE"
    fi

    # Kill any remaining chat2response processes on port 8099
    lsof -ti:8099 | xargs kill -9 2>/dev/null || true
}

# Trap EXIT to ensure cleanup runs
trap cleanup EXIT INT TERM

#####################################################################
# INSTALLATION FUNCTIONS
#####################################################################

check_uv_installed() {
    if command -v uv &> /dev/null; then
        log_success "uv is already installed: $(uv --version)"
        return 0
    else
        return 1
    fi
}

install_uv() {
    log_info "Installing uv..."

    if [[ "$OSTYPE" == "darwin"* ]] || [[ "$OSTYPE" == "linux-gnu"* ]]; then
        curl -LsSf https://astral.sh/uv/install.sh | sh

        # Add to PATH for this session
        export PATH="$HOME/.cargo/bin:$PATH"

        if check_uv_installed; then
            log_success "uv installed successfully"
        else
            log_error "Failed to install uv"
            exit 1
        fi
    else
        log_error "Unsupported OS: $OSTYPE"
        log_info "Please install uv manually from https://github.com/astral-sh/uv"
        exit 1
    fi
}

check_rust_installed() {
    if command -v cargo &> /dev/null; then
        log_success "Rust is installed: $(cargo --version)"
        return 0
    else
        log_error "Rust is not installed"
        log_info "Install Rust from https://rustup.rs/"
        return 1
    fi
}

build_chat2response() {
    log_info "Building chat2response server..."
    cd "$PROJECT_ROOT"

    if cargo build --release; then
        log_success "chat2response built successfully"
    else
        log_error "Failed to build chat2response"
        exit 1
    fi
}

#####################################################################
# ENVIRONMENT SETUP
#####################################################################

check_env_file() {
    if [ ! -f "$ENV_FILE" ]; then
        log_error ".env file not found at $ENV_FILE"
        exit 1
    fi

    log_success "Found .env file"

    # Validate required environment variables
    if ! grep -q "OPENAI_API_KEY" "$ENV_FILE" || ! grep -q "CHAT2RESPONSE_BASE" "$ENV_FILE"; then
        log_warning ".env file may be missing required variables"
    fi
}

setup_python_env() {
    log_info "Setting up Python environment with uv..."
    cd "$PYTHON_TESTS_DIR"

    # Create virtual environment using uv
    log_info "Creating virtual environment..."
    uv venv --clear

    # Activate virtual environment
    source .venv/bin/activate

    # Install dependencies
    log_info "Installing dependencies..."
    uv pip install -e .

    log_success "Python environment setup complete"
}

#####################################################################
# SERVER MANAGEMENT
#####################################################################

start_chat2response_server() {
    log_info "Starting chat2response server..."

    cd "$PROJECT_ROOT"

    # Load environment variables properly handling spaces and quotes
    while IFS= read -r line; do
        # Skip comments and empty lines
        [[ $line =~ ^#.*$ ]] && continue
        [[ -z $line ]] && continue

        # Extract key and value
        if [[ $line =~ ^([A-Z_]+)=(.*)$ ]]; then
            key="${BASH_REMATCH[1]}"
            value="${BASH_REMATCH[2]}"
            # Remove quotes if present
            value=$(echo "$value" | sed -e 's/^"//' -e 's/"$//')
            export "$key=$value"
        fi
    done < "$ENV_FILE"

    # Start server in background
    ./target/release/chat2response &
    SERVER_PID=$!

    # Save PID
    echo "$SERVER_PID" > "$SERVER_PID_FILE"

    log_info "chat2response server started (PID: $SERVER_PID)"

    # Wait for server to be ready
    log_info "Waiting for server to be ready..."
    MAX_WAIT=30
    WAITED=0

    while [ $WAITED -lt $MAX_WAIT ]; do
        if curl -s "http://127.0.0.1:8099/status" > /dev/null 2>&1; then
            log_success "Server is ready"
            return 0
        fi
        sleep 1
        WAITED=$((WAITED + 1))
    done

    log_error "Server failed to start within ${MAX_WAIT}s"
    exit 1
}

#####################################################################
# TEST EXECUTION
#####################################################################

run_tests() {
    log_info "Running integration tests..."

    cd "$PYTHON_TESTS_DIR"

    # Ensure virtual environment is activated
    if [ -z "${VIRTUAL_ENV:-}" ]; then
        source .venv/bin/activate
    fi

    # Load environment variables for tests
    while IFS= read -r line; do
        # Skip comments and empty lines
        [[ $line =~ ^#.*$ ]] && continue
        [[ -z $line ]] && continue

        # Extract key and value
        if [[ $line =~ ^([A-Z_]+)=(.*)$ ]]; then
            key="${BASH_REMATCH[1]}"
            value="${BASH_REMATCH[2]}"
            # Remove quotes if present
            value=$(echo "$value" | sed -e 's/^"//' -e 's/"$//')
            export "$key=$value"
        fi
    done < "$ENV_FILE"

    # Run pytest with verbose output
    if pytest tests/ -v -s --tb=short; then
        log_success "All tests passed!"
        return 0
    else
        log_error "Some tests failed"
        return 1
    fi
}

#####################################################################
# MAIN EXECUTION
#####################################################################

main() {
    log_info "Starting chat2response Python integration test setup"
    echo ""

    # Step 1: Check and install uv
    log_info "Step 1: Checking uv installation"
    if ! check_uv_installed; then
        install_uv
    fi
    echo ""

    # Step 2: Check Rust installation
    log_info "Step 2: Checking Rust installation"
    if ! check_rust_installed; then
        exit 1
    fi
    echo ""

    # Step 3: Check environment file
    log_info "Step 3: Validating environment configuration"
    check_env_file
    echo ""

    # Step 4: Build chat2response
    log_info "Step 4: Building chat2response server"
    build_chat2response
    echo ""

    # Step 5: Setup Python environment
    log_info "Step 5: Setting up Python test environment"
    setup_python_env
    echo ""

    # Step 6: Start server
    log_info "Step 6: Starting chat2response server"
    start_chat2response_server
    echo ""

    # Step 7: Run tests
    log_info "Step 7: Running integration tests"
    if run_tests; then
        echo ""
        log_success "====================================="
        log_success "All setup and tests completed successfully!"
        log_success "====================================="
        exit 0
    else
        echo ""
        log_error "====================================="
        log_error "Tests failed - see output above"
        log_error "====================================="
        exit 1
    fi
}

# Run main function
main "$@"
