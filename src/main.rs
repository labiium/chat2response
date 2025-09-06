use chat2response::server::build_router;
use chat2response::util::{env_bind_addr, init_tracing};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    init_tracing();

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
