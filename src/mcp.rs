/* Minimal MCP stdio server stub.

This stub is always compiled and provides the symbol `run_stdio_server` expected by
the main binary. It does not implement the MCP protocol. Instead, when invoked
(typically when CHAT2RESPONSE_MCP=1), it blocks by reading stdin until EOF and then
returns Ok(()). This preserves the run-flow without introducing extra dependencies
or compile-time breakage.

If you need a real MCP server, replace this stub with a concrete implementation
using your preferred MCP SDK. Keep the function signature intact.

Runtime behavior:
- Logs startup/shutdown via `tracing` (if configured).
- Reads and discards lines from stdin until it reaches EOF.
*/

use anyhow::Result;
use tokio::io::{self, AsyncBufReadExt, BufReader};

/// Start a no-op MCP "server" over stdio and block until stdin closes.
pub async fn run_stdio_server() -> Result<()> {
    tracing::info!(
        "CHAT2RESPONSE MCP: starting stub stdio server (no-op). Waiting for EOF on stdin..."
    );
    let mut reader = BufReader::new(io::stdin());
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF
        }
        // Intentionally ignore input; this is a stub.
    }

    tracing::info!("CHAT2RESPONSE MCP: stdin closed; exiting stub server.");
    Ok(())
}
