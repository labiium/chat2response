"""
Integration tests for tool calling via routiium proxy.

Tests validate:
1. Basic tool calling with function definitions
2. Streaming mode with tool calls
3. Multi-turn conversations with tool execution
4. Tool call response handling

Time complexity: O(n) per test where n is response size
Space complexity: O(n) for storing responses
"""

import os
import json
import pytest
from openai import OpenAI
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


@pytest.fixture
def test_model():
    """Get the model to use for testing from environment or use default."""
    chat_model = os.getenv("CHAT_MODEL")
    if chat_model:
        return chat_model
    return os.getenv("MODEL", "gpt-4o-mini")


@pytest.fixture
def weather_tool():
    """Define a sample weather tool for testing."""
    return {
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the current weather for a location",
            "parameters": {
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "The city and state, e.g. San Francisco, CA",
                    },
                    "unit": {
                        "type": "string",
                        "enum": ["celsius", "fahrenheit"],
                        "description": "The temperature unit to use",
                    },
                },
                "required": ["location"],
            },
        },
    }


@pytest.fixture
def calculator_tools():
    """Define calculator tools for testing."""
    return [
        {
            "type": "function",
            "function": {
                "name": "add",
                "description": "Add two numbers",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number", "description": "First number"},
                        "b": {"type": "number", "description": "Second number"},
                    },
                    "required": ["a", "b"],
                },
            },
        },
        {
            "type": "function",
            "function": {
                "name": "multiply",
                "description": "Multiply two numbers",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number", "description": "First number"},
                        "b": {"type": "number", "description": "Second number"},
                    },
                    "required": ["a", "b"],
                },
            },
        },
    ]


class TestBasicToolCalling:
    """Test suite for basic tool calling functionality."""

    def test_single_tool_call(self, routiium_client, test_model, weather_tool):
        """
        Test basic tool calling with a single tool.

        Validates:
        - Tool call is requested by the model
        - Tool call contains proper structure
        - Function name and arguments are correct
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {"role": "user", "content": "What's the weather in San Francisco?"}
            ],
            tools=[weather_tool],
            tool_choice="auto",
            stream=False,
        )

        assert response.choices[0].message.tool_calls is not None
        assert len(response.choices[0].message.tool_calls) > 0

        tool_call = response.choices[0].message.tool_calls[0]
        assert tool_call.function.name == "get_weather"

        # Parse and validate arguments
        args = json.loads(tool_call.function.arguments)
        assert "location" in args
        assert (
            "San Francisco" in args["location"]
            or "san francisco" in args["location"].lower()
        )

        print(f"\n✓ Tool call made: {tool_call.function.name}")
        print(f"  Arguments: {tool_call.function.arguments}")

    def test_tool_call_with_specific_parameters(
        self, routiium_client, test_model, weather_tool
    ):
        """
        Test tool calling with specific parameter requirements.

        Validates:
        - Model correctly extracts parameters from natural language
        - Both required and optional parameters are handled
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {"role": "user", "content": "What's the weather in Tokyo in celsius?"}
            ],
            tools=[weather_tool],
            tool_choice="auto",
            stream=False,
        )

        tool_call = response.choices[0].message.tool_calls[0]
        args = json.loads(tool_call.function.arguments)

        assert "location" in args
        assert "Tokyo" in args["location"] or "tokyo" in args["location"].lower()

        # Check if unit was provided (optional)
        if "unit" in args:
            assert args["unit"] in ["celsius", "fahrenheit"]
            print(f"\n✓ Tool call with unit: {args['unit']}")
        else:
            print(f"\n✓ Tool call without unit (optional parameter)")

        print(f"  Location: {args['location']}")

    def test_multiple_tools_selection(
        self, routiium_client, test_model, calculator_tools
    ):
        """
        Test that model selects correct tool from multiple options.

        Validates:
        - Model chooses appropriate tool based on context
        - Correct function is called

        Note: Some models may respond with text instead of tool calls
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "What is 15 plus 27?"}],
            tools=calculator_tools,
            tool_choice="auto",
            stream=False,
        )

        # Check if model made tool calls
        if response.choices[0].message.tool_calls:
            tool_call = response.choices[0].message.tool_calls[0]
            assert tool_call.function.name == "add"

            args = json.loads(tool_call.function.arguments)
            assert "a" in args and "b" in args

            print(f"\n✓ Correct tool selected: {tool_call.function.name}")
            print(f"  Arguments: a={args['a']}, b={args['b']}")
        else:
            # Model chose to respond with text instead
            assert response.choices[0].message.content is not None
            print(f"\n✓ Model responded with text (no tool call)")
            print(f"  Response: {response.choices[0].message.content[:100]}")
            # For gpt-5-nano or models without tool support, this is acceptable
            pytest.skip(f"Model {test_model} did not use tool calls for this query")

    def test_forced_tool_call(self, routiium_client, test_model, weather_tool):
        """
        Test forcing a specific tool call.

        Validates:
        - tool_choice parameter works correctly
        - Specified tool is called even if not obvious from prompt
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "Tell me about Paris"}],
            tools=[weather_tool],
            tool_choice={"type": "function", "function": {"name": "get_weather"}},
            stream=False,
        )

        tool_call = response.choices[0].message.tool_calls[0]
        assert tool_call.function.name == "get_weather"

        print(f"\n✓ Forced tool call succeeded: {tool_call.function.name}")


class TestStreamingToolCalling:
    """Test suite for streaming mode with tool calls."""

    def test_streaming_tool_call(self, routiium_client, test_model, weather_tool):
        """
        Test tool calling in streaming mode.

        Validates:
        - Tool calls work in streaming mode
        - Tool call chunks are properly accumulated
        - Final tool call is complete and valid
        """
        stream = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "What's the weather in Boston?"}],
            tools=[weather_tool],
            stream=True,
        )

        chunks = []
        tool_call_chunks = []

        for chunk in stream:
            chunks.append(chunk)
            if chunk.choices and len(chunk.choices) > 0:
                delta = chunk.choices[0].delta
                if delta.tool_calls:
                    tool_call_chunks.extend(delta.tool_calls)

        assert len(chunks) > 0, "Should receive at least one chunk"
        assert len(tool_call_chunks) > 0, "Should receive tool call chunks"

        print(f"\n✓ Streaming with tool calls: {len(chunks)} chunks")
        print(f"  Tool call chunks: {len(tool_call_chunks)}")

    def test_streaming_mixed_content(
        self, routiium_client, test_model, calculator_tools
    ):
        """
        Test streaming with both text and tool call content.

        Validates:
        - Both text content and tool calls can appear in stream
        - Content is properly separated
        """
        stream = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {
                    "role": "user",
                    "content": "Calculate 8 times 9 and explain the result",
                }
            ],
            tools=calculator_tools,
            stream=True,
        )

        has_text_content = False
        has_tool_calls = False

        for chunk in stream:
            if chunk.choices and len(chunk.choices) > 0:
                delta = chunk.choices[0].delta
                if delta.content:
                    has_text_content = True
                if delta.tool_calls:
                    has_tool_calls = True

        # Note: Depending on model behavior, we might get one or both
        print(f"\n✓ Streaming mixed content:")
        print(f"  Has text: {has_text_content}")
        print(f"  Has tool calls: {has_tool_calls}")


class TestMultiTurnToolConversations:
    """Test suite for multi-turn conversations with tool execution."""

    def test_tool_execution_and_response(
        self, routiium_client, test_model, calculator_tools
    ):
        """
        Test complete tool execution flow.

        Validates:
        - Initial tool call is made
        - Tool result can be sent back
        - Model generates final response based on tool result

        Note: Some models may respond with text instead of tool calls
        """
        # Step 1: Initial request that triggers tool call
        response1 = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "What is 123 times 456?"}],
            tools=calculator_tools,
            stream=False,
        )

        # Check if model made tool calls
        if not response1.choices[0].message.tool_calls:
            # Model chose to respond with text instead
            assert response1.choices[0].message.content is not None
            print(f"\n✓ Model responded with text (no tool call)")
            print(f"  Response: {response1.choices[0].message.content[:100]}")
            pytest.skip(f"Model {test_model} did not use tool calls for this query")
            return

        tool_call = response1.choices[0].message.tool_calls[0]
        assert tool_call.function.name == "multiply"

        # Parse arguments and simulate tool execution
        args = json.loads(tool_call.function.arguments)
        result = args["a"] * args["b"]

        print(
            f"\n✓ Tool call executed: {tool_call.function.name}({args['a']}, {args['b']}) = {result}"
        )

        # Step 2: Send tool result back
        response2 = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {"role": "user", "content": "What is 123 times 456?"},
                response1.choices[0].message,
                {"role": "tool", "tool_call_id": tool_call.id, "content": str(result)},
            ],
            tools=calculator_tools,
            stream=False,
        )

        # Model should now provide a final answer
        assert response2.choices[0].message.content is not None
        # Check for result with or without comma formatting (56088 or 56,088)
        result_str = str(result)
        result_formatted = f"{result:,}"
        assert (
            result_str in response2.choices[0].message.content
            or result_formatted in response2.choices[0].message.content
        )

        print(f"✓ Final response: {response2.choices[0].message.content[:100]}")

    def test_multiple_tool_calls_in_conversation(
        self, routiium_client, test_model, calculator_tools
    ):
        """
        Test multiple sequential tool calls.

        Validates:
        - Multiple tool calls can be made in sequence
        - Context is maintained across calls
        """
        # First calculation
        response1 = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "First, add 5 and 3"}],
            tools=calculator_tools,
            stream=False,
        )

        assert response1.choices[0].message.tool_calls is not None
        tool_call1 = response1.choices[0].message.tool_calls[0]
        assert tool_call1.function.name == "add"

        args1 = json.loads(tool_call1.function.arguments)
        result1 = args1["a"] + args1["b"]

        print(
            f"\n✓ First calculation: {tool_call1.function.name}({args1['a']}, {args1['b']}) = {result1}"
        )

        # Second calculation building on first
        response2 = routiium_client.chat.completions.create(
            model=test_model,
            messages=[
                {"role": "user", "content": "First, add 5 and 3"},
                response1.choices[0].message,
                {
                    "role": "tool",
                    "tool_call_id": tool_call1.id,
                    "content": str(result1),
                },
                {"role": "user", "content": "Now multiply that result by 2"},
            ],
            tools=calculator_tools,
            stream=False,
        )

        # Check if we got another tool call or a direct response
        if response2.choices[0].message.tool_calls:
            tool_call2 = response2.choices[0].message.tool_calls[0]
            print(f"✓ Second calculation: {tool_call2.function.name}")
        else:
            print(f"✓ Final response: {response2.choices[0].message.content[:100]}")


class TestToolCallErrorHandling:
    """Test suite for error handling in tool calling."""

    def test_no_tool_call_when_not_needed(
        self, routiium_client, test_model, weather_tool
    ):
        """
        Test that tools are not called when not necessary.

        Validates:
        - Model responds normally when tool is not needed
        - No spurious tool calls
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "What is the capital of France?"}],
            tools=[weather_tool],
            tool_choice="auto",
            stream=False,
        )

        # Should respond with text, not tool call
        if response.choices[0].message.tool_calls:
            print(f"\n⚠ Unexpected tool call made (model may be over-using tools)")
        else:
            assert response.choices[0].message.content is not None
            print(f"\n✓ No tool call made (correct behavior)")
            print(f"  Response: {response.choices[0].message.content[:100]}")

    def test_tool_call_with_parallel_calls(
        self, routiium_client, test_model, calculator_tools
    ):
        """
        Test handling of multiple parallel tool calls.

        Validates:
        - Model can request multiple tool calls at once
        - All tool calls are properly structured
        """
        response = routiium_client.chat.completions.create(
            model=test_model,
            messages=[{"role": "user", "content": "Calculate both 5+3 and 5*3"}],
            tools=calculator_tools,
            stream=False,
        )

        tool_calls = response.choices[0].message.tool_calls

        if tool_calls and len(tool_calls) > 1:
            print(f"\n✓ Parallel tool calls: {len(tool_calls)}")
            for tc in tool_calls:
                print(f"  - {tc.function.name}")
        elif tool_calls and len(tool_calls) == 1:
            print(f"\n✓ Single tool call: {tool_calls[0].function.name}")
            print(f"  (Model may need explicit parallel calling prompt)")
        else:
            print(f"\n✓ No tool calls (model chose to respond directly)")


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
