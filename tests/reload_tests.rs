use actix_web::{test, web, App};
use chat2response::server::config_routes;
use chat2response::util::AppState;
use serde_json::json;
use std::sync::Arc;

#[actix_web::test]
async fn test_reload_mcp_without_path() {
    let app_state = AppState::default();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/reload/mcp")
        .insert_header(("content-type", "application/json"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 400);

    let body = test::read_body(resp).await;
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(body_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No MCP config path"));
}

#[actix_web::test]
async fn test_reload_system_prompt_without_path() {
    let app_state = AppState::default();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/reload/system_prompt")
        .insert_header(("content-type", "application/json"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 400);

    let body = test::read_body(resp).await;
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(body_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("No system prompt config path"));
}

#[actix_web::test]
async fn test_reload_all_without_paths() {
    let app_state = AppState::default();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/reload/all")
        .insert_header(("content-type", "application/json"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 200);

    let body = test::read_body(resp).await;
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body_json["mcp"]["success"], false);
    assert_eq!(body_json["system_prompt"]["success"], false);
}

#[actix_web::test]
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

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/reload/system_prompt")
        .insert_header(("content-type", "application/json"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 200);

    let body = test::read_body(resp).await;
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body_json["success"], true);
    assert_eq!(body_json["enabled"], true);
    assert_eq!(body_json["has_global"], true);
    assert_eq!(body_json["injection_mode"], "prepend");
}

#[actix_web::test]
async fn test_reload_system_prompt_with_invalid_config() {
    // Create a temporary invalid config file
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("invalid.json");

    std::fs::write(&config_path, "{ invalid json }").unwrap();

    let mut app_state = AppState::default();
    app_state.system_prompt_config_path = Some(config_path.to_string_lossy().to_string());

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/reload/system_prompt")
        .insert_header(("content-type", "application/json"))
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 500);
}

#[actix_web::test]
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

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let chat_request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let req = test::TestRequest::post()
        .uri("/convert")
        .insert_header(("content-type", "application/json"))
        .set_payload(serde_json::to_string(&chat_request).unwrap())
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 200);

    let body = test::read_body(resp).await;
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check that system prompt was injected
    // In Responses API format, messages are under "input"
    let messages = body_json["input"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are a helpful AI assistant");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Hello");
}

#[actix_web::test]
async fn test_status_endpoint_includes_new_routes() {
    let app_state = AppState::default();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(app_state))
            .configure(config_routes),
    )
    .await;

    let req = test::TestRequest::get().uri("/status").to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), 200);

    let body = test::read_body(resp).await;
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
