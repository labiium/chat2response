#!/usr/bin/env python3
"""FastMCP-based mock MCP server for integration tests."""

from fastmcp import FastMCP


mcp = FastMCP("Routiium Mock MCP")


@mcp.tool(name="echo")
def mock_echo(text: str) -> str:
    """Echo the provided text with a friendly prefix."""
    return f"Echo: {text}"


if __name__ == "__main__":
    mcp.run()
