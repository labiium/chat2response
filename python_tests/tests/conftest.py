"""
Pytest configuration and shared fixtures for routiium integration tests.

This module sets up authentication tokens for testing in managed mode.
"""

import os
import pytest
import requests
from dotenv import load_dotenv


# Load environment variables
load_dotenv(dotenv_path=os.path.join(os.path.dirname(__file__), "../../.env"))


@pytest.fixture(scope="session", autouse=True)
def setup_test_api_key():
    """
    Generate a test API key for use in managed authentication mode.

    This fixture runs once per test session and generates a temporary
    access token that all tests can use. The token is stored in an
    environment variable.
    """
    base_url = os.getenv("ROUTIIUM_BASE", "http://127.0.0.1:8099")

    # Generate a temporary access token
    try:
        response = requests.post(
            f"{base_url}/keys/generate",
            json={"label": "pytest-session", "ttl_seconds": 3600},
            timeout=5,
        )
        response.raise_for_status()

        key_data = response.json()
        access_token = key_data.get("token")

        if not access_token:
            pytest.exit(
                f"Failed to generate test access token: no token in response: {key_data}"
            )

        # Store the access token for tests to use
        os.environ["ROUTIIUM_ACCESS_TOKEN"] = access_token
        print(f"\n✓ Generated test access token: {access_token[:20]}...")

        yield access_token

    except Exception as e:
        pytest.exit(f"Failed to generate test access token: {e}")
