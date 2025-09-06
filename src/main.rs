use chat2response::server::build_router;
use chat2response::util::{env_bind_addr, init_tracing};
use std::net::SocketAddr;
use tokio::net::TcpListener;

mod mcp;

/// Returns true if MCP mode is requested via env and the stdio server is started.
/// If false, the HTTP server should be started instead.
async fn maybe_run_mcp() -> bool {
    if !env_mcp_requested() {
        return false;
    }
    try_run_stdio_server().await
}

fn env_mcp_requested() -> bool {
    match std::env::var("CHAT2RESPONSE_MCP") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes"
        }
        Err(_) => false,
    }
}

async fn try_run_stdio_server() -> bool {
    match mcp::run_stdio_server().await {
        Ok(_) => true, // MCP server ran and terminated; exit process path
        Err(e) => {
            eprintln!("MCP server failed: {e:#}");
            false
        }
    }
}

#[tokio::main]
async fn main() {
    init_tracing();

    // If MCP mode was requested and successfully started, exit when it ends.
    if maybe_run_mcp().await {
        return;
    }

    // Default: start HTTP server
    let addr: SocketAddr = env_bind_addr()
        .parse()
        .expect("Invalid BIND_ADDR (expected host:port)");
    let app = build_router();
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");
    tracing::info!(
        "Chat2Response listening on http://{}",
        listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| env_bind_addr())
    );
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("server error: {e:#}");
    }
}
