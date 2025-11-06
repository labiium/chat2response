#!/usr/bin/env python3
"""
Probe OpenAI Python SDK for Responses API support.

This script checks if the OpenAI Python SDK has native support for the
Responses API endpoint (/v1/responses) that routiium proxies.
"""

import sys

try:
    import openai
    from openai import OpenAI
except ImportError:
    print("ERROR: openai package not installed")
    print("Install with: pip install openai>=1.0.0")
    sys.exit(1)

print(f"OpenAI SDK Version: {openai.__version__}")
print("=" * 60)

# Create client instance
client = OpenAI(api_key="dummy-key-for-inspection")

print("\nTop-level client attributes:")
print("-" * 60)
client_attrs = [x for x in dir(client) if not x.startswith("_")]
for attr in client_attrs[:30]:
    print(f"  - {attr}")
if len(client_attrs) > 30:
    print(f"  ... and {len(client_attrs) - 30} more")

print("\n\nChecking for Responses API:")
print("-" * 60)

# Check for responses attribute
if hasattr(client, "responses"):
    print("✅ client.responses EXISTS")
    responses_obj = getattr(client, "responses")
    print(f"   Type: {type(responses_obj)}")
    print(f"   Methods: {[x for x in dir(responses_obj) if not x.startswith('_')]}")

    if hasattr(responses_obj, "create"):
        print("✅ client.responses.create() EXISTS")
        print("\n🎉 OpenAI SDK HAS NATIVE RESPONSES API SUPPORT!")
        print("   We should use client.responses.create() in tests")
    else:
        print("❌ client.responses.create() NOT FOUND")
else:
    print("❌ client.responses NOT FOUND")
    print("\n⚠️  OpenAI SDK does NOT have native Responses API support")
    print("   We must use httpx for direct HTTP calls to /v1/responses")

# Check for beta features
print("\n\nChecking for beta features:")
print("-" * 60)
if hasattr(client, "beta"):
    print("✅ client.beta EXISTS")
    beta_obj = getattr(client, "beta")
    beta_attrs = [x for x in dir(beta_obj) if not x.startswith("_")]
    print(f"   Beta attributes: {beta_attrs[:10]}")

    if hasattr(beta_obj, "responses"):
        print("✅ client.beta.responses EXISTS (beta feature)")
        print("\n🎉 Responses API available as BETA feature!")
        print("   We should use client.beta.responses.create() in tests")
else:
    print("❌ client.beta NOT FOUND")

# Check all top-level resources
print("\n\nAll client resources:")
print("-" * 60)
resources = []
for attr in dir(client):
    if not attr.startswith("_"):
        obj = getattr(client, attr)
        if not callable(obj) and hasattr(obj, "__class__"):
            resources.append(attr)

for resource in resources:
    print(f"  - client.{resource}")

print("\n" + "=" * 60)
print("RECOMMENDATION:")
print("=" * 60)

if hasattr(client, "responses") or (
    hasattr(client, "beta") and hasattr(client.beta, "responses")
):
    print("✅ Use native OpenAI SDK for Responses API testing")
    if hasattr(client, "responses"):
        print("   Code: client.responses.create(...)")
    else:
        print("   Code: client.beta.responses.create(...)")
else:
    print("❌ OpenAI SDK does not support Responses API")
    print("   Solution: Use httpx for direct HTTP calls")
    print("   Code: httpx_client.post('/v1/responses', json=payload)")

print("\n")
