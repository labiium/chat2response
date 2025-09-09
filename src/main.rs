use chat2response::auth::ApiKeyManager;
use chat2response::mcp_client::McpClientManager;
use chat2response::mcp_config::McpConfig;
use chat2response::server::build_router_with_state;
use chat2response::util::{build_http_client_from_env, env_bind_addr, init_tracing, AppState};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    init_tracing();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Initialize key backend and optional MCP config from CLI args
    let backend_opt = ApiKeyManager::backend_from_args(&args);
    let api_keys = match backend_opt {
        Some(backend) => match ApiKeyManager::from_backend(backend) {
            Ok(mgr) => {
                tracing::info!("API key backend initialized from CLI");
                Some(Arc::new(mgr))
            }
            Err(e) => {
                tracing::error!("Failed to initialize API key backend from CLI: {}", e);
                None
            }
        },
        None => {
            // Env-based fallback (Redis URL -> Redis, else sled if available, else memory)
            match ApiKeyManager::new_default() {
                Ok(mgr) => Some(Arc::new(mgr)),
                Err(e) => {
                    tracing::warn!("Falling back to no API key manager: {}", e);
                    None
                }
            }
        }
    };

    // Find first non-flag positional arg as MCP config path (optional)
    let mcp_config_arg = args.iter().skip(1).find(|a| !a.starts_with('-')).cloned();

    let mcp_manager_arc: Option<Arc<McpClientManager>> =
        if let Some(mcp_config_path) = mcp_config_arg {
            tracing::info!("Loading MCP configuration from: {}", mcp_config_path);
            match McpConfig::load_from_file(&mcp_config_path) {
                Ok(config) => {
                    tracing::info!("Found {} MCP servers in config", config.mcp_servers.len());
                    match McpClientManager::new(config).await {
                        Ok(manager) => {
                            tracing::info!("Successfully initialized MCP client manager");
                            Some(Arc::new(manager))
                        }
                        Err(e) => {
                            tracing::error!("Failed to initialize MCP client manager: {}", e);
                            tracing::warn!("Continuing without MCP support");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load MCP config: {}", e);
                    tracing::warn!("Continuing without MCP support");
                    None
                }
            }
        } else {
            tracing::info!("No MCP config provided, running without MCP support");
            tracing::info!(
                "Usage: {} [mcp.json] [--keys-backend=redis://...|sled:<path>|memory]",
                args[0]
            );
            None
        };

    let app_state = AppState {
        http: build_http_client_from_env(),
        mcp_manager: mcp_manager_arc,
        api_keys,
    };
    // Startup mode announcement (managed vs passthrough)
    let managed_mode = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_some();
    if managed_mode {
        tracing::info!("Auth mode: managed (internal upstream key; client bearer tokens validated and substituted upstream)");
    } else {
        tracing::info!("Auth mode: passthrough (client bearer tokens forwarded upstream)");
    }

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
