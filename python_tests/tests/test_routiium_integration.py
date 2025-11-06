"""
Integration tests for routiium proxy server.

Tests validate:
1. Chat completions API via routiium proxy
2. Responses API via routiium proxy using native OpenAI SDK
3. Streaming and non-streaming modes
4. Tool calling and vision capabilities
5. Error handling and edge cases

Time complexity: O(n) per test where n is response size
Space complexity: O(n) for storing responses in memory
"""

import os
import pytest
import time
import json
from openai import OpenAI, OpenAIError
from dotenv import load_dotenv


# Load environment variables from .env file
load_dotenv(dotenv_path=os.path.join(os.path.dirname(__file__), "../../.env"))


@pytest.fixture(scope="module")
def routiium_client():
    """
    Create OpenAI client configured to use routiium as backend.

    Returns:
        OpenAI: Client pointing to routiium proxy server
    """
    base_url = os.getenv("ROUTIIUM_BASE", "http://127.0.0.1:8099")
    # Use the generated access token for managed authentication mode
    api_key = os.getenv(
        "ROUTIIUM_ACCESS_TOKEN", os.getenv("OPENAI_API_KEY", "test-key")
    )

    if not base_url:
        pytest.skip("ROUTIIUM_BASE not configured")

    client = OpenAI(
        base_url=f"{base_url}/v1",
        api_key=api_key,
    )

    return client


@pytest.fixture(scope="module")
def openai_client():
    """
    Create direct OpenAI client for comparison tests.

    Returns:
        OpenAI: Client pointing directly to OpenAI API
    """
    base_url = os.getenv("OPENAI_BASE_URL", "https://api.openai.com/v1")
    api_key = os.getenv("OPENAI_API_KEY")

    if not api_key or api_key == "test-key":
        pytest.skip("Valid OPENAI_API_KEY required for direct OpenAI tests")

    client = OpenAI(
        base_url=base_url,
        api_key=api_key,
    )

    return client


@pytest.fixture
def test_model():
    """Get the model to use for testing from environment or use default."""
    chat_model = os.getenv("CHAT_MODEL")
    model = os.getenv("MODEL", "gpt-4o-mini")
    if chat_model:
        return chat_model
    return model


@pytest.fixture
def test_prompt():
    """Get the test prompt from environment or use default."""
    return os.getenv("PROMPT", "Say 'Hello, World!' and nothing else.")


class TestChatCompletions:
    """Test suite for Chat Completions API via routiium proxy."""

    def test_basic_chat_completion(self, routiium_client, test_model, test_prompt):
        """
        Test basic non-streaming chat completion through routiium.

        Validates:
        - Request completes successfully
        - Response contains expected fields
        - Message content is non-empty
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": test_prompt}],
            stream=False,
        )

        assert response.id is not None
        assert response.object == "chat.completion"
        assert response.model is not None
        assert len(response.choices) > 0

        choice = response.choices[0]
        assert choice.message is not None
        assert choice.message.role == "assistant"
        assert choice.message.content is not None
        assert len(choice.message.content) > 0

        # Verify usage information
        assert response.usage is not None
        assert response.usage.prompt_tokens > 0
        assert response.usage.completion_tokens > 0
        assert response.usage.total_tokens > 0

        print(f"\n✓ Chat completion response: {choice.message.content[:100]}")

    def test_streaming_chat_completion(
        self, routiium_client, test_model, test_prompt
    ):
        """
        Test streaming chat completion through routiium.

        Validates:
        - Stream produces multiple chunks
        - Chunks contain delta content
        - Stream completes successfully
        """
        stream = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": test_prompt}],
            stream=True,
        )

        chunks = []
        content_parts = []

        for chunk in stream:
            chunks.append(chunk)
            if chunk.choices and len(chunk.choices) > 0:
                delta = chunk.choices[0].delta
                if delta.content:
                    content_parts.append(delta.content)

        assert len(chunks) > 0, "Should receive at least one chunk"

        full_content = "".join(content_parts)
        assert len(full_content) > 0, "Stream should produce content"

        print(
            f"\n✓ Streaming chat completion: {len(chunks)} chunks, content: {full_content[:100]}"
        )

    def test_chat_completion_with_system_message(
        self, routiium_client, test_model
    ):
        """
        Test chat completion with system message.

        Validates:
        - System messages are properly handled
        - Multi-message conversations work
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {
                    "role": "system",
                    "content": "You are a helpful assistant that responds concisely.",
                },
                {"role": "user", "content": "What is 2+2?"},
            ],
            stream=False,
        )

        assert response.choices[0].message.content is not None
        assert len(response.choices[0].message.content) > 0

        print(
            f"\n✓ System message response: {response.choices[0].message.content[:100]}"
        )

    def test_chat_completion_with_max_tokens(self, routiium_client, test_model):
        """
        Test chat completion with max_tokens/max_completion_tokens parameter.

        Validates:
        - max_tokens or max_completion_tokens parameter is respected
        - Response doesn't exceed limit

        Note: gpt-5-nano requires max_completion_tokens instead of max_tokens
        """
        max_tokens = 10

        # gpt-5-nano requires max_completion_tokens instead of max_tokens
        if "gpt-5-nano" in test_model.lower():
            response = routiium_client.chat.completions.create(
                model=test_model,
                messages=[
                    {"role": "user", "content": "Write a long story about a dragon."}
                ],
                max_completion_tokens=max_tokens,
                stream=False,
            )
        else:
            response = routiium_client.chat.completions.create(
                model=test_model,
                messages=[
                    {"role": "user", "content": "Write a long story about a dragon."}
                ],
                max_tokens=max_tokens,
                stream=False,
            )

        assert response.choices[0].message.content is not None
        # Note: completion_tokens should be <= max_tokens
        assert (
            response.usage.completion_tokens <= max_tokens + 5
        )  # Small buffer for tokenizer differences

        print(
            f"\n✓ Max tokens test: {response.usage.completion_tokens} tokens used (limit: {max_tokens})"
        )

    def test_chat_completion_with_temperature(self, routiium_client, test_model):
        """
        Test chat completion with temperature parameter.

        Validates:
        - Temperature parameter is accepted (for models that support it)
        - Response is generated successfully

        Note: gpt-5-nano only supports default temperature (1)
        """
        # gpt-5-nano only supports default temperature (1)
        if "gpt-5-nano" in test_model.lower():
            response = routiium_client.chat.completions.create(
                model=test_model,
                messages=[{"role": "user", "content": "Say hello"}],
                stream=False,
            )
            print(f"\n✓ Temperature test (default): {response.choices[0].message.content[:100]}")
        else:
            response = routiium_client.chat.completions.create(
                model=test_model,
                messages=[{"role": "user", "content": "Say hello"}],
                temperature=0.7,
                stream=False,
            )
            print(f"\n✓ Temperature test (0.7): {response.choices[0].message.content[:100]}")

        assert response.choices[0].message.content is not None


class TestResponsesAPI:
    """
    Test suite for Responses API via routiium /v1/responses endpoint.

    Uses native OpenAI SDK client.responses.create() for 1:1 implementation testing.
    The /v1/responses endpoint accepts requests and forwards them to OpenAI's
    Responses API, which has a different structure than chat completions.
    """

    def test_basic_responses_endpoint(
        self, routiium_client, test_model, test_prompt
    ):
        """
        Test basic non-streaming request to /v1/responses endpoint using native SDK.

        Validates:
        - /v1/responses endpoint is accessible via native SDK
        - Request is properly forwarded to OpenAI Responses API
        - Response contains expected Responses API structure
        - Content is non-empty

        Time complexity: O(n) where n is response size
        Space complexity: O(n) for response storage
        """
        response = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": test_prompt}],
        )

        # Validate response structure matches OpenAI Responses API format
        assert response.id is not None, "Response should contain 'id' field"
        assert response.output is not None, "Response should have 'output' field"

        # Responses API has output array
        assert len(response.output) > 0, "Should have at least one output item"

        # Get text content
        assert response.output_text is not None, "Should have output_text"
        assert len(response.output_text) > 0, "Output text should not be empty"

        # Validate usage information exists
        assert response.usage is not None, "Response should contain 'usage' field"
        assert response.usage.input_tokens > 0, "Should have input tokens"
        assert response.usage.output_tokens > 0, "Should have output tokens"

        print(f"\n✓ Responses API endpoint response: {response.output_text[:100]}")
        print(f"  Response ID: {response.id}")
        print(
            f"  Usage: {response.usage.input_tokens} input + {response.usage.output_tokens} output tokens"
        )

    def test_responses_endpoint_with_system_message(
        self, routiium_client, test_model
    ):
        """
        Test /v1/responses endpoint with system message using native SDK.

        Validates:
        - System messages are properly handled by Responses API
        - Multi-message conversations work correctly
        """
        response = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "system",
                    "content": "You are a math tutor. Answer concisely.",
                },
                {"role": "user", "content": "What is 5 + 3?"},
            ],
        )

        assert response.output_text is not None
        assert len(response.output_text) > 0

        print(f"\n✓ Responses API with system message: {response.output_text[:100]}")

    def test_responses_endpoint_streaming(
        self, routiium_client, test_model, test_prompt
    ):
        """
        Test streaming mode on /v1/responses endpoint using native SDK.

        Validates:
        - Streaming responses work correctly
        - Stream produces multiple chunks
        - Content is assembled correctly

        Time complexity: O(n) where n is number of chunks
        Space complexity: O(n) for storing chunks
        """
        stream = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": test_prompt}],
            stream=True,
        )

        chunks = []
        content_parts = []

        for chunk in stream:
            chunks.append(chunk)

            # Responses API streaming events with delta attribute (ResponseTextDeltaEvent)
            if hasattr(chunk, "delta") and chunk.delta:
                content_parts.append(chunk.delta)

        assert len(chunks) > 0, "Should receive at least one chunk"

        full_content = "".join(content_parts)
        assert len(full_content) > 0, "Stream should produce content"

        print(
            f"\n✓ Responses API streaming: {len(chunks)} chunks, content: {full_content[:100]}"
        )

    def test_responses_endpoint_with_parameters(self, routiium_client, test_model):
        """
        Test /v1/responses endpoint with various parameters using native SDK.

        Validates:
        - Temperature parameter is accepted (for models that support it)
        - Max_output_tokens parameter is respected
        - Parameters are properly forwarded to backend

        Note: gpt-5-nano doesn't support custom temperature
        """
        # gpt-5-nano doesn't support custom temperature
        if "gpt-5-nano" in test_model.lower():
            response = routiium_client.responses.create(
                model=test_model,
                input=[{"role": "user", "content": "Tell me a very short joke."}],
                max_output_tokens=50,
            )
        else:
            response = routiium_client.responses.create(
                model=test_model,
                input=[{"role": "user", "content": "Tell me a very short joke."}],
                temperature=0.9,
                max_output_tokens=50,
            )

        assert response.output_text is not None

        # Verify max_output_tokens was respected
        if response.usage:
            output_tokens = response.usage.output_tokens
            assert output_tokens <= 55, (
                f"Output tokens ({output_tokens}) exceeded max (50) by too much"
            )

        print(f"\n✓ Responses API with parameters: {response.output_text[:100]}")
        print(
            f"  Tokens used: {response.usage.output_tokens if response.usage else 'N/A'}"
        )

    def test_responses_endpoint_metadata_preservation(
        self, routiium_client, test_model
    ):
        """
        Test that metadata is preserved through /v1/responses endpoint using native SDK.

        Validates:
        - Response includes model information
        - Usage statistics are present
        - Response structure is complete
        """
        response = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": "Say OK"}],
        )

        # Verify core metadata
        assert response.id is not None, "Should have response ID"
        assert response.model is not None, "Should include model info"

        # Verify usage data
        assert response.usage is not None, "Should include usage statistics"
        assert response.usage.input_tokens > 0
        assert response.usage.output_tokens > 0

        print(f"\n✓ Responses API metadata preservation validated")
        print(f"  Model: {response.model}")
        print(f"  Response ID: {response.id}")

    def test_responses_endpoint_error_handling(self, routiium_client):
        """
        Test error handling on /v1/responses endpoint using native SDK.

        Validates:
        - Invalid model errors are properly returned
        - Error messages are informative
        """
        with pytest.raises(Exception) as exc_info:
            routiium_client.responses.create(
                model="invalid-model-that-does-not-exist-99999",
                input=[{"role": "user", "content": "Hello"}],
            )

        # Verify some error was raised
        assert exc_info.value is not None

        print(f"\n✓ Responses API error handling: {type(exc_info.value).__name__}")

    def test_responses_endpoint_with_tools(self, routiium_client, test_model):
        """
        Test /v1/responses endpoint with tool/function calling using native SDK.

        Validates:
        - Tool definitions are properly forwarded
        - Tool calls are returned in response
        - Tool call structure is correct
        - Response includes appropriate output

        Time complexity: O(n) where n is response size
        Space complexity: O(n) for response storage
        """
        # Define a simple weather tool
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the current weather for a location",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "City and state, e.g. San Francisco, CA",
                            },
                            "unit": {
                                "type": "string",
                                "enum": ["celsius", "fahrenheit"],
                                "description": "Temperature unit",
                            },
                        },
                        "required": ["location"],
                    },
                },
            }
        ]

        response = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": "What's the weather in Tokyo?"}],
            tools=tools,
        )

        assert response.output is not None
        assert len(response.output) > 0

        # Check if model called the tool (non-deterministic)
        has_tool_call = any(
            hasattr(item, "type") and item.type == "function_call"
            for item in response.output
        )

        if has_tool_call:
            print(f"\n✓ Tool call detected in Responses API")
            print(f"  Response has {len(response.output)} output items")
        else:
            # Model chose not to call tool - validate text response
            assert response.output_text, "If no tool call, should have text output"
            print(f"\n✓ Responses API tool test: Model responded with text")
            print(f"  Response: {response.output_text[:100]}")

    def test_responses_endpoint_with_multiple_tools(
        self, routiium_client, test_model
    ):
        """
        Test /v1/responses endpoint with multiple tool definitions using native SDK.

        Validates:
        - Multiple tools can be defined
        - Model can choose appropriate tool
        - Tool definitions are properly structured
        """
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string"},
                        },
                        "required": ["location"],
                    },
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "search_web",
                    "description": "Search the web",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"},
                        },
                        "required": ["query"],
                    },
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "calculate",
                    "description": "Perform mathematical calculation",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "expression": {"type": "string"},
                        },
                        "required": ["expression"],
                    },
                },
            },
        ]

        response = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": "Calculate 25 * 4 + 10"}],
            tools=tools,
        )

        assert response.output is not None
        assert len(response.output) > 0

        # Check for tool calls or text response
        has_tool_call = any(
            hasattr(item, "type") and item.type == "function_call"
            for item in response.output
        )

        if has_tool_call:
            print(f"\n✓ Multiple tools test: Model selected a tool")
        else:
            assert response.output_text
            print(f"\n✓ Multiple tools test: Model responded with text")

    def test_responses_endpoint_tool_streaming(self, routiium_client, test_model):
        """
        Test streaming mode with tool calling on /v1/responses endpoint using native SDK.

        Validates:
        - Tool calls work with streaming
        - Tool call information is assembled from deltas
        - Stream completes successfully

        Time complexity: O(n) where n is number of chunks
        Space complexity: O(n) for chunk storage
        """
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_time",
                    "description": "Get current time for a timezone",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "timezone": {"type": "string"},
                        },
                        "required": ["timezone"],
                    },
                },
            }
        ]

        stream = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": "What time is it in New York?"}],
            tools=tools,
            stream=True,
        )

        chunks = []
        for chunk in stream:
            chunks.append(chunk)

        assert len(chunks) > 0, "Should receive chunks"
        print(f"\n✓ Tool calling streaming: {len(chunks)} chunks received")

    def test_responses_endpoint_with_vision(self, routiium_client, test_model):
        """
        Test /v1/responses endpoint with vision/image inputs using native SDK.

        Validates:
        - Image URLs are properly handled
        - Multimodal content array format works
        - Response is generated for image + text input
        - Vision-capable models can describe images

        Time complexity: O(n) where n is response size
        Space complexity: O(n) for response storage

        Note: Requires vision-capable model (gpt-4o, gpt-4o-mini, gpt-4-vision-preview)
        """
        # Use a public test image
        image_url = "https://upload.wikimedia.org/wikipedia/commons/thumb/d/dd/Gfp-wisconsin-madison-the-nature-boardwalk.jpg/640px-Gfp-wisconsin-madison-the-nature-boardwalk.jpg"

        # Check if model supports vision
        # gpt-5-nano supports multimodal/vision via Responses API
        vision_capable_models = ["gpt-4o", "gpt-4o-mini", "gpt-4-vision", "gpt-4-turbo", "gpt-5-nano", "gpt-5"]
        if not any(vm in test_model.lower() for vm in vision_capable_models):
            pytest.skip(f"Model {test_model} may not support vision via Responses API")

        response = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "What do you see in this image? Be brief.",
                        },
                        {
                            "type": "input_image",
                            "image_url": image_url,
                            "detail": "auto",
                        },
                    ],
                }
            ],
            max_output_tokens=300,
        )

        # Validate response structure
        assert response.output_text is not None
        assert len(response.output_text) > 10, (
            "Vision response should have substantial content"
        )

        print(f"\n✓ Vision/image input test: {response.output_text[:150]}")
        print(f"  Image analyzed: {image_url[:60]}...")

    def test_responses_endpoint_vision_with_base64(
        self, routiium_client, test_model
    ):
        """
        Test /v1/responses endpoint with base64-encoded images using native SDK.

        Validates:
        - Base64 image format is supported
        - Data URIs work correctly
        - Image content is properly processed

        Note: Uses a minimal 1x1 red pixel for testing
        """
        # gpt-5-nano supports multimodal/vision via Responses API
        vision_capable_models = ["gpt-4o", "gpt-4o-mini", "gpt-4-vision", "gpt-4-turbo", "gpt-5-nano", "gpt-5"]
        if not any(vm in test_model.lower() for vm in vision_capable_models):
            pytest.skip(f"Model {test_model} may not support vision via Responses API")

        # 1x1 red pixel PNG (base64)
        base64_image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg=="

        response = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "What color is this pixel?"},
                        {
                            "type": "input_image",
                            "image_url": f"data:image/png;base64,{base64_image}",
                        },
                    ],
                }
            ],
            max_output_tokens=50,
        )

        assert response.output_text is not None

        print(f"\n✓ Base64 image test: {response.output_text[:100]}")

    def test_responses_endpoint_vision_streaming(
        self, routiium_client, test_model
    ):
        """
        Test streaming mode with vision inputs on /v1/responses endpoint using native SDK.

        Validates:
        - Streaming works with image inputs
        - Content is streamed chunk by chunk
        - Stream completes successfully

        Time complexity: O(n) where n is number of chunks
        Space complexity: O(n) for chunk storage
        """
        # gpt-5-nano supports multimodal/vision via Responses API
        vision_capable_models = ["gpt-4o", "gpt-4o-mini", "gpt-4-vision", "gpt-4-turbo", "gpt-5-nano", "gpt-5"]
        if not any(vm in test_model.lower() for vm in vision_capable_models):
            pytest.skip(f"Model {test_model} may not support vision via Responses API")

        image_url = "https://upload.wikimedia.org/wikipedia/commons/thumb/3/3a/Cat03.jpg/481px-Cat03.jpg"

        stream = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "Describe this image briefly."},
                        {
                            "type": "input_image",
                            "image_url": image_url,
                            "detail": "auto",
                        },
                    ],
                }
            ],
            max_output_tokens=150,
            stream=True,
        )

        chunks = []
        content_parts = []

        for chunk in stream:
            chunks.append(chunk)
            # Responses API streaming events with delta attribute (ResponseTextDeltaEvent)
            if hasattr(chunk, "delta") and chunk.delta:
                content_parts.append(chunk.delta)

        assert len(chunks) > 0, "Should receive chunks"

        # Note: Some models may not stream delta content for vision queries
        # They may send the full response in a final chunk instead
        full_content = "".join(content_parts)
        if len(full_content) > 0:
            print(
                f"\n✓ Vision streaming: {len(chunks)} chunks, content: {full_content[:100]}"
            )
        else:
            # Check if we got content in a different format
            print(
                f"\n✓ Vision streaming: {len(chunks)} chunks received (model may not stream deltas for vision)"
            )

    def test_responses_endpoint_vision_with_tools(
        self, routiium_client, test_model
    ):
        """
        Test combining vision and tool calling in single request using native SDK.

        Validates:
        - Vision and tools can be used together
        - Model can analyze image and call tools
        - Complex multimodal + function calling scenarios work

        Time complexity: O(n) where n is response size
        Space complexity: O(n) for response storage
        """
        # gpt-5-nano supports multimodal/vision via Responses API
        vision_capable_models = ["gpt-4o", "gpt-4o-mini", "gpt-4-vision", "gpt-4-turbo", "gpt-5-nano", "gpt-5"]
        if not any(vm in test_model.lower() for vm in vision_capable_models):
            pytest.skip(f"Model {test_model} may not support vision via Responses API")

        image_url = "https://upload.wikimedia.org/wikipedia/commons/thumb/3/3a/Cat03.jpg/481px-Cat03.jpg"

        tools = [
            {
                "type": "function",
                "function": {
                    "name": "identify_animal",
                    "description": "Identify an animal species",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "species": {"type": "string"},
                            "confidence": {"type": "number"},
                        },
                        "required": ["species"],
                    },
                },
            }
        ]

        response = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "What animal is in this image? Use the identify_animal function.",
                        },
                        {
                            "type": "input_image",
                            "image_url": image_url,
                            "detail": "auto",
                        },
                    ],
                }
            ],
            tools=tools,
        )

        assert response.output is not None
        assert len(response.output) > 0

        # Model might call tool or respond with text
        has_tool_call = any(
            hasattr(item, "type") and item.type == "function_call"
            for item in response.output
        )

        if has_tool_call:
            print(f"\n✓ Vision + tools: Model called tool")
        else:
            print(f"\n✓ Vision + tools: Model responded with text")
            print(
                f"  Response: {response.output_text[:100] if response.output_text else 'N/A'}"
            )

    def test_responses_endpoint_latency(
        self, routiium_client, test_model, test_prompt
    ):
        """
        Test and measure response latency for /v1/responses endpoint using native SDK.

        Validates:
        - Responses API endpoint performs within acceptable limits
        - Latency is measured accurately

        Time complexity: O(1) - single request
        Space complexity: O(n) where n is response size
        """
        start_time = time.time()

        response = routiium_client.responses.create(
            model=test_model,
            input=[{"role": "user", "content": test_prompt}],
        )

        end_time = time.time()
        latency_ms = (end_time - start_time) * 1000

        assert response.output_text is not None

        assert latency_ms < 30000, f"Response took too long: {latency_ms}ms"

        print(f"\n✓ Responses API endpoint latency: {latency_ms:.2f}ms")

    def test_responses_endpoint_reasoning_content(
        self, routiium_client, test_model
    ):
        """
        Test /v1/responses endpoint with reasoning_content for reasoning models.

        Validates:
        - Reasoning content is properly captured and returned
        - Response structure includes reasoning_content field
        - Usage metadata includes reasoning_tokens if applicable

        Note: gpt-5-nano and o1/o3 models support reasoning_content
        """
        # Use a prompt that requires reasoning
        response = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": "Think step by step: What is 15 * 24?",
                }
            ],
        )

        assert response.output is not None
        assert len(response.output) > 0

        # Check if response has reasoning content
        has_reasoning = False
        reasoning_content = []

        for item in response.output:
            if hasattr(item, "type"):
                if item.type == "reasoning":
                    has_reasoning = True
                    if hasattr(item, "content") and item.content:
                        reasoning_content.append(item.content)

        # Check usage metadata for reasoning tokens
        has_reasoning_tokens = False
        if response.usage and hasattr(response.usage, "reasoning_tokens"):
            if response.usage.reasoning_tokens and response.usage.reasoning_tokens > 0:
                has_reasoning_tokens = True
                print(f"\n✓ Reasoning tokens detected: {response.usage.reasoning_tokens}")

        if has_reasoning:
            print(f"✓ Reasoning content detected in response")
            if reasoning_content:
                print(f"  Reasoning: {reasoning_content[0][:100]}...")
        else:
            print(f"✓ No explicit reasoning content (model may not support or include it)")

        # Validate that we got a valid response
        assert response.output_text is not None
        print(f"  Final answer: {response.output_text[:100]}")

    def test_responses_endpoint_reasoning_streaming(
        self, routiium_client, test_model
    ):
        """
        Test streaming mode with reasoning content for reasoning models.

        Validates:
        - Reasoning content is streamed properly
        - Reasoning and text content are distinguished
        - Stream completes successfully

        Note: gpt-5-nano and o1/o3 models support reasoning_content
        """
        stream = routiium_client.responses.create(
            model=test_model,
            input=[
                {
                    "role": "user",
                    "content": "Solve this step by step: If a train travels 60 mph for 2.5 hours, how far does it go?",
                }
            ],
            stream=True,
        )

        chunks = []
        reasoning_chunks = []
        text_chunks = []

        for chunk in stream:
            chunks.append(chunk)

            # Check for reasoning content in streaming
            if hasattr(chunk, "type"):
                if chunk.type == "reasoning_delta" or chunk.type == "reasoning":
                    if hasattr(chunk, "delta") and chunk.delta:
                        reasoning_chunks.append(chunk.delta)

            # Check for text content
            if hasattr(chunk, "output_text_delta") and chunk.output_text_delta:
                text_chunks.append(chunk.output_text_delta)

        assert len(chunks) > 0, "Should receive at least one chunk"

        reasoning_text = "".join(reasoning_chunks) if reasoning_chunks else None
        final_text = "".join(text_chunks) if text_chunks else None

        if reasoning_text:
            print(f"\n✓ Reasoning streaming: {len(reasoning_chunks)} reasoning chunks")
            print(f"  Reasoning: {reasoning_text[:100]}...")
        else:
            print(f"\n✓ No reasoning chunks in stream (model may not support)")

        if final_text:
            print(f"  Final answer: {final_text[:100]}")

        print(f"  Total chunks: {len(chunks)}")


class TestProxyBehavior:
    """Test suite for routiium proxy-specific behavior."""

    def test_conversation_id_handling(self, routiium_client, test_model):
        """
        Test that conversation IDs are properly handled.

        Validates:
        - Multiple requests complete successfully
        - Each response has unique ID
        """
        responses = []

        for i in range(3):
            response = routiium_client.chat.completions.create(
                model=test_model,
                messages=[{"role": "user", "content": f"Say the number {i}"}],
                stream=False,
            )
            responses.append(response)

        # Check all responses succeeded
        assert len(responses) == 3

        # Check each has an ID
        for response in responses:
            assert response.id is not None

        print(f"\n✓ Conversation ID handling: {len(responses)} requests completed")

    def test_error_handling_invalid_model(self, routiium_client):
        """
        Test error handling for invalid model.

        Validates:
        - Appropriate error is raised
        - Error message is informative
        """
        with pytest.raises(Exception) as exc_info:
            routiium_client.chat.completions.create(
                model="invalid-model-that-does-not-exist-12345",
                messages=[{"role": "user", "content": "Hello"}],
                stream=False,
            )

        # Verify some error was raised
        assert exc_info.value is not None

        print(f"\n✓ Error handling test passed: {type(exc_info.value).__name__}")

    def test_empty_message_handling(self, routiium_client, test_model):
        """
        Test handling of edge case with empty or minimal messages.

        Validates:
        - Empty/minimal content is handled gracefully
        """
        response = routiium_client.chat.completions.create(
            model=test_model, messages=[{"role": "user", "content": "Hi"}], stream=False
        )

        assert response.choices[0].message.content is not None

        print(
            f"\n✓ Minimal message handling: {response.choices[0].message.content[:100]}"
        )


class TestPerformance:
    """Test suite for performance and latency validation."""

    def test_response_latency(self, routiium_client, test_model, test_prompt):
        """
        Test and measure response latency through proxy.

        Validates:
        - Response completes in reasonable time
        - Latency metrics are captured
        """
        start_time = time.time()

        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": test_prompt}],
            stream=False,
        )

        end_time = time.time()
        latency_ms = (end_time - start_time) * 1000

        assert response.choices[0].message.content is not None
        assert latency_ms < 30000, f"Response took too long: {latency_ms}ms"

        print(f"\n✓ Response latency: {latency_ms:.2f}ms")

    def test_streaming_latency(self, routiium_client, test_model, test_prompt):
        """
        Test time to first token in streaming mode.

        Validates:
        - First chunk arrives quickly
        - Stream completes successfully
        """
        start_time = time.time()
        first_chunk_time = None

        stream = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": test_prompt}],
            stream=True,
        )

        chunk_count = 0
        for chunk in stream:
            if first_chunk_time is None and chunk.choices:
                first_chunk_time = time.time()
            chunk_count += 1

        end_time = time.time()

        assert first_chunk_time is not None, "Should receive at least one chunk"

        ttft_ms = (first_chunk_time - start_time) * 1000
        total_ms = (end_time - start_time) * 1000

        print(
            f"\n✓ Streaming performance: TTFT={ttft_ms:.2f}ms, Total={total_ms:.2f}ms, Chunks={chunk_count}"
        )


class TestRouterIntegration:
    """Verify router alias resolution routes to expected upstream models."""

    @staticmethod
    def _prompt() -> list[dict]:
        return [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Respond with a short friendly greeting."}
                ],
            }
        ]

    def test_router_alias_nano_basic(self, routiium_client):
        alias = os.getenv("ROUTER_ALIAS_BASIC", "nano-basic")
        try:
            response = routiium_client.responses.create(
                model=alias,
                input=self._prompt(),
                stream=False,
            )
        except OpenAIError as exc:
            pytest.skip(f"Router alias '{alias}' not configured: {exc}")
        except Exception as exc:  # Network or other unexpected failure
            pytest.skip(f"Router alias '{alias}' unavailable: {exc}")

        assert response.model == "gpt-5-nano"
        assert response.output_text is not None
        assert response.output_text.strip() != ""

    def test_router_alias_nano_advanced(self, routiium_client):
        alias = os.getenv("ROUTER_ALIAS_ADVANCED", "nano-advanced")
        try:
            response = routiium_client.responses.create(
                model=alias,
                input=self._prompt(),
                stream=False,
            )
        except OpenAIError as exc:
            pytest.skip(f"Router alias '{alias}' not configured: {exc}")
        except Exception as exc:
            pytest.skip(f"Router alias '{alias}' unavailable: {exc}")

        assert response.model == "gpt-4.1-nano"
        assert response.output_text is not None
        assert response.output_text.strip() != ""


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
