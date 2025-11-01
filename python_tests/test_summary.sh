#!/bin/bash
echo "========================================="
echo "CHAT2RESPONSE TEST SUMMARY"
echo "========================================="
echo ""
echo "Running all tests..."
echo ""
source .venv/bin/activate
pytest tests/ -v --tb=short 2>&1 | grep -E "(PASSED|FAILED|ERROR|passed|failed|error)" | tail -30
echo ""
echo "========================================="
