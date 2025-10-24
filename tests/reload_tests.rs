use axum::http::StatusCode;
use chat2response::server::build_router_with_state;
use chat2response::util::AppState;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_reload_mcp_without_path() {
    let app_state = AppState::default();
    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/reload/mcp")
        .header("content-type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(body_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No MCP config path"));
}

#[tokio::test]
async fn test_reload_system_prompt_without_path() {
    let app_state = AppState::default();
    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/reload/system_prompt")
        .header("content-type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(body_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No system prompt config path"));
}

#[tokio::test]
async fn test_reload_all_without_paths() {
    let app_state = AppState::default();
    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/reload/all")
        .header("content-type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body_json["mcp"]["success"], false);
    assert_eq!(body_json["system_prompt"]["success"], false);
}

#[tokio::test]
async fn test_reload_system_prompt_with_valid_config() {
    // Create a temporary config file
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("system_prompt.json");

    let config = json!({
        "global": "You are a helpful assistant",
        "enabled": true,
        "injection_mode": "prepend"
    });

    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    let mut app_state = AppState::default();
    app_state.system_prompt_config_path = Some(config_path.to_string_lossy().to_string());

    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/reload/system_prompt")
        .header("content-type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body_json["success"], true);
    assert_eq!(body_json["enabled"], true);
    assert_eq!(body_json["has_global"], true);
    assert_eq!(body_json["injection_mode"], "prepend");
}

#[tokio::test]
async fn test_reload_system_prompt_with_invalid_config() {
    // Create a temporary invalid config file
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("invalid.json");

    std::fs::write(&config_path, "{ invalid json }").unwrap();

    let mut app_state = AppState::default();
    app_state.system_prompt_config_path = Some(config_path.to_string_lossy().to_string());

    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/reload/system_prompt")
        .header("content-type", "application/json")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_convert_with_system_prompt() {
    // Create a temporary config file
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("system_prompt.json");

    let config = json!({
        "global": "You are a helpful AI assistant",
        "enabled": true,
        "injection_mode": "prepend"
    });

    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    let mut app_state = AppState::default();
    app_state.system_prompt_config_path = Some(config_path.to_string_lossy().to_string());

    // Load the config
    let loaded_config =
        chat2response::system_prompt_config::SystemPromptConfig::load_from_file(&config_path)
            .unwrap();
    app_state.system_prompt_config = Arc::new(tokio::sync::RwLock::new(loaded_config));

    let app = build_router_with_state(app_state);

    let chat_request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/convert")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&chat_request).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check that system prompt was injected
    let messages = body_json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are a helpful AI assistant");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Hello");
}

#[tokio::test]
async fn test_status_endpoint_includes_new_routes() {
    let app_state = AppState::default();
    let app = build_router_with_state(app_state);

    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/status")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let routes = body_json["routes"].as_array().unwrap();
    let route_strings: Vec<String> = routes
        .iter()
        .map(|r| r.as_str().unwrap().to_string())
        .collect();

    // Verify reload routes are present
    assert!(route_strings.contains(&"/reload/mcp".to_string()));
    assert!(route_strings.contains(&"/reload/system_prompt".to_string()));
    assert!(route_strings.contains(&"/reload/all".to_string()));
}
