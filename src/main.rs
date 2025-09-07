use chat2response::mcp_client::McpClientManager;
use chat2response::mcp_config::McpConfig;
use chat2response::server::build_router_with_state;
use chat2response::util::{env_bind_addr, init_tracing, AppState};
use std::env;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    init_tracing();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Initialize MCP manager if mcp.json path is provided
    let app_state = if args.len() > 1 {
        let mcp_config_path = &args[1];
        tracing::info!("Loading MCP configuration from: {}", mcp_config_path);

        match McpConfig::load_from_file(mcp_config_path) {
            Ok(config) => {
                tracing::info!("Found {} MCP servers in config", config.mcp_servers.len());

                match McpClientManager::new(config).await {
                    Ok(manager) => {
                        tracing::info!("Successfully initialized MCP client manager");
                        AppState::with_mcp_manager(manager)
                    }
                    Err(e) => {
                        tracing::error!("Failed to initialize MCP client manager: {}", e);
                        tracing::warn!("Continuing without MCP support");
                        AppState::default()
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to load MCP config: {}", e);
                tracing::warn!("Continuing without MCP support");
                AppState::default()
            }
        }
    } else {
        tracing::info!("No MCP config provided, running without MCP support");
        tracing::info!("Usage: {} [mcp.json]", args[0]);
        AppState::default()
    };

    let addr: SocketAddr = env_bind_addr()
        .parse()
        .expect("Invalid BIND_ADDR (expected host:port)");
    let app = build_router_with_state(app_state);
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
