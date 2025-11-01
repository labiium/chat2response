#!/usr/bin/env bash
#####################################################################
# API KEY VALIDATION SCRIPT
#####################################################################
#
# This script validates the OPENAI_API_KEY in .env file
# and provides guidance for fixing authentication issues.
#
#####################################################################

set -e
set -u

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENV_FILE="$PROJECT_ROOT/.env"

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

echo ""
echo "=========================================="
echo "  OpenAI API Key Validation"
echo "=========================================="
echo ""

# Check .env file exists
if [ ! -f "$ENV_FILE" ]; then
    log_error ".env file not found at $ENV_FILE"
    exit 1
fi

log_info "Found .env file at: $ENV_FILE"

# Extract API key
API_KEY=$(grep "^OPENAI_API_KEY=" "$ENV_FILE" | cut -d= -f2 | tr -d '"' | tr -d "'")

if [ -z "$API_KEY" ]; then
    log_error "OPENAI_API_KEY not found in .env file"
    echo ""
    echo "Add the following line to $ENV_FILE:"
    echo ""
    echo "  OPENAI_API_KEY=sk-proj-your-actual-api-key-here"
    echo ""
    exit 1
fi

log_info "API key found in .env file"

# Validate key format
if [[ ! "$API_KEY" =~ ^sk- ]]; then
    log_warning "API key doesn't start with 'sk-' - may be invalid format"
fi

# Test API key with OpenAI
log_info "Testing API key with OpenAI API..."
echo ""

HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer $API_KEY" \
    https://api.openai.com/v1/models 2>/dev/null || echo "000")

if [ "$HTTP_CODE" = "200" ]; then
    log_success "✓ API key is VALID and working!"
    echo ""
    echo "You can now run tests with:"
    echo "  cd python_tests"
    echo "  ./setup_and_test.sh"
    echo ""
    exit 0
elif [ "$HTTP_CODE" = "401" ]; then
    log_error "✗ API key is INVALID or expired"
    echo ""
    echo "The API key in your .env file is not valid."
    echo ""
    echo "To fix this:"
    echo "1. Go to https://platform.openai.com/api-keys"
    echo "2. Create a new API key"
    echo "3. Update the OPENAI_API_KEY in: $ENV_FILE"
    echo "4. Run this script again to validate"
    echo ""
    exit 1
elif [ "$HTTP_CODE" = "429" ]; then
    log_warning "Rate limit reached or quota exceeded"
    echo ""
    echo "Your API key is valid but you've hit a rate limit or quota."
    echo "Check your usage at: https://platform.openai.com/usage"
    echo ""
    exit 1
elif [ "$HTTP_CODE" = "000" ]; then
    log_error "Network error - could not reach OpenAI API"
    echo ""
    echo "Check your internet connection and try again."
    echo ""
    exit 1
else
    log_error "Unexpected response code: $HTTP_CODE"
    echo ""
    echo "Something unexpected happened. Try again or check OpenAI status."
    echo ""
    exit 1
fi
