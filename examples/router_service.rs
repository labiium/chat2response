//! Example Router Service (v0.3)
//!
//! A minimal Actix-web implementation of the Router API v0.3.
//! This demonstrates the Router service contract that Chat2Response integrates with.
//!
//! Run with:
//! ```bash
//! cargo run --example router_service
//! ```
//!
//! Then test with Chat2Response:
//! ```bash
//! CHAT2R_ROUTER_URL=http://localhost:9090 chat2response
//! ```

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use std::sync::{Arc, Mutex};

// Re-use types from chat2response
use chat2response::router_client::{
    CacheControl, Capabilities, CatalogModel, CostCard, ModelCatalog, ModelLimits, PolicyInfo,
    PromptOverlays, RecentMetrics, RouteFeedback, RouteHints, RouteLimits, RoutePlan, RouteRequest,
    SLOs, Stickiness, UpstreamConfig, UpstreamMode,
};

/// Shared router state
struct RouterState {
    /// Model catalog
    catalog: ModelCatalog,
    /// Catalog revision counter
    catalog_revision: Arc<Mutex<u32>>,
    /// Feedback history
    feedback_log: Arc<Mutex<Vec<RouteFeedback>>>,
    /// Request counter for route IDs
    route_counter: Arc<Mutex<u64>>,
}

impl RouterState {
    fn new() -> Self {
        // Build a simple model catalog
        let models = vec![
            CatalogModel {
                id: "gpt-4o-mini-2024-07-18".to_string(),
                provider: "openai".to_string(),
                region: Some(vec!["us-east-1".to_string(), "eu-west-1".to_string()]),
                aliases: vec!["tier:T1".to_string(), "family:gpt-4o-mini".to_string()],
                capabilities: Capabilities {
                    modalities: vec!["text".to_string(), "image".to_string()],
                    context_tokens: Some(128000),
                    tools: true,
                    json_mode: true,
                    prompt_cache: true,
                    logprobs: false,
                    structured_output: true,
                },
                usage_notes: Some(
                    "Great cost/latency balance; ideal for hints & code drafts.".to_string(),
                ),
                cost: CostCard {
                    currency: "USD".to_string(),
                    input_per_million: Some(0.15),
                    output_per_million: Some(0.60),
                    cached_per_million: Some(0.075),
                    reasoning_per_million: None,
                    input_per_million_micro: Some(150_000),
                    output_per_million_micro: Some(600_000),
                    cached_per_million_micro: Some(75_000),
                    reasoning_per_million_micro: None,
                },
                slos: SLOs {
                    target_p95_ms: Some(3000),
                    recent: Some(RecentMetrics {
                        p50_ms: Some(700),
                        p95_ms: Some(2100),
                        error_rate: Some(0.003),
                        tokens_per_sec: Some(450.0),
                    }),
                },
                limits: Some(ModelLimits {
                    tps: Some(20),
                    rpm: Some(1800),
                    rps_burst: Some(10),
                }),
                policy_tags: vec!["T1".to_string(), "edu_safe".to_string()],
                status: "healthy".to_string(),
                status_reason: None,
                deprecates_at: None,
                rl_policy: Some("default".to_string()),
                deprecated: Some(false),
            },
            CatalogModel {
                id: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                region: Some(vec!["us-east-1".to_string(), "eu-west-1".to_string()]),
                aliases: vec!["tier:T2".to_string(), "family:gpt-4o".to_string()],
                capabilities: Capabilities {
                    modalities: vec!["text".to_string(), "image".to_string()],
                    context_tokens: Some(128000),
                    tools: true,
                    json_mode: true,
                    prompt_cache: true,
                    logprobs: true,
                    structured_output: true,
                },
                usage_notes: Some("High quality reasoning; use for complex tasks.".to_string()),
                cost: CostCard {
                    currency: "USD".to_string(),
                    input_per_million: Some(2.50),
                    output_per_million: Some(10.0),
                    cached_per_million: Some(1.25),
                    reasoning_per_million: None,
                    input_per_million_micro: Some(2_500_000),
                    output_per_million_micro: Some(10_000_000),
                    cached_per_million_micro: Some(1_250_000),
                    reasoning_per_million_micro: None,
                },
                slos: SLOs {
                    target_p95_ms: Some(5000),
                    recent: Some(RecentMetrics {
                        p50_ms: Some(1200),
                        p95_ms: Some(3500),
                        error_rate: Some(0.001),
                        tokens_per_sec: Some(380.0),
                    }),
                },
                limits: Some(ModelLimits {
                    tps: Some(10),
                    rpm: Some(1200),
                    rps_burst: Some(6),
                }),
                policy_tags: vec!["T2".to_string(), "edu_safe".to_string()],
                status: "healthy".to_string(),
                status_reason: None,
                deprecates_at: None,
                rl_policy: Some("balanced".to_string()),
                deprecated: Some(false),
            },
            CatalogModel {
                id: "llama-3.1-70b-instruct".to_string(),
                provider: "local-vllm".to_string(),
                region: Some(vec!["local".to_string()]),
                aliases: vec!["tier:T1".to_string(), "family:llama".to_string()],
                capabilities: Capabilities {
                    modalities: vec!["text".to_string()],
                    context_tokens: Some(8192),
                    tools: true,
                    json_mode: true,
                    prompt_cache: false,
                    logprobs: false,
                    structured_output: false,
                },
                usage_notes: Some(
                    "Local inference; fast and free but limited context.".to_string(),
                ),
                cost: CostCard {
                    currency: "USD".to_string(),
                    input_per_million: Some(0.0),
                    output_per_million: Some(0.0),
                    cached_per_million: Some(0.0),
                    reasoning_per_million: None,
                    input_per_million_micro: Some(0),
                    output_per_million_micro: Some(0),
                    cached_per_million_micro: Some(0),
                    reasoning_per_million_micro: None,
                },
                slos: SLOs {
                    target_p95_ms: Some(2000),
                    recent: Some(RecentMetrics {
                        p50_ms: Some(400),
                        p95_ms: Some(1200),
                        error_rate: Some(0.010),
                        tokens_per_sec: Some(600.0),
                    }),
                },
                limits: Some(ModelLimits {
                    tps: Some(30),
                    rpm: Some(3600),
                    rps_burst: Some(20),
                }),
                policy_tags: vec!["T1".to_string(), "offline_ok".to_string()],
                status: "healthy".to_string(),
                status_reason: None,
                deprecates_at: None,
                rl_policy: Some("local".to_string()),
                deprecated: Some(false),
            },
        ];

        Self {
            catalog: ModelCatalog {
                revision: "cat_v1".to_string(),
                models,
            },
            catalog_revision: Arc::new(Mutex::new(1)),
            feedback_log: Arc::new(Mutex::new(Vec::new())),
            route_counter: Arc::new(Mutex::new(1)),
        }
    }

    fn next_route_id(&self) -> String {
        let mut counter = self.route_counter.lock().unwrap();
        let id = *counter;
        *counter += 1;
        format!("rte_{:016x}", id)
    }

    fn policy_revision(&self) -> String {
        let rev = self.catalog_revision.lock().unwrap();
        format!("pol_v{}", *rev)
    }
}

/// GET /catalog/models - Return model catalog
async fn get_catalog(state: web::Data<RouterState>, req: HttpRequest) -> impl Responder {
    let start = std::time::Instant::now();

    // Check If-None-Match for ETag caching
    if let Some(etag) = req.headers().get("if-none-match") {
        if etag.to_str().unwrap_or("") == format!("\"{}\"", state.catalog.revision).as_str() {
            return HttpResponse::NotModified().finish();
        }
    }

    let response = HttpResponse::Ok()
        .insert_header(("ETag", format!("\"{}\"", state.catalog.revision)))
        .insert_header(("Cache-Control", "public, max-age=30"))
        .insert_header((
            "Router-Latency",
            format!("{}ms", start.elapsed().as_millis()),
        ))
        .json(&state.catalog);

    response
}

/// POST /route/plan - Make routing decision
async fn route_plan(state: web::Data<RouterState>, req: web::Json<RouteRequest>) -> impl Responder {
    let start = std::time::Instant::now();
    let route_req = req.into_inner();

    println!(
        "🔀 Route request: alias={}, api={}, privacy={:?}, caps={:?}",
        route_req.alias, route_req.api, route_req.privacy_mode, route_req.caps
    );

    // Simple routing policy: map aliases to models
    let (model_id, provider) = match route_req.alias.as_str() {
        "labiium-001" | "edu-hint" => ("gpt-4o-mini-2024-07-18", "openai"),
        "labiium-smart" | "edu-solution" => ("gpt-4o", "openai"),
        "labiium-local" => ("llama-3.1-70b-instruct", "local-vllm"),
        _ => {
            // Default to gpt-4o-mini
            ("gpt-4o-mini-2024-07-18", "openai")
        }
    };

    // Find model in catalog for cost/SLO info
    let catalog_model = state
        .catalog
        .models
        .iter()
        .find(|m| m.id == model_id)
        .cloned();

    // Build upstream config
    let (base_url, auth_env) = match provider {
        "openai" => (
            "https://api.openai.com/v1".to_string(),
            Some("OPENAI_API_KEY".to_string()),
        ),
        "local-vllm" => ("http://localhost:8000/v1".to_string(), None),
        _ => (
            "https://api.openai.com/v1".to_string(),
            Some("OPENAI_API_KEY".to_string()),
        ),
    };

    let upstream = UpstreamConfig {
        base_url,
        mode: if provider == "local-vllm" {
            UpstreamMode::Chat
        } else {
            UpstreamMode::Responses
        },
        model_id: model_id.to_string(),
        auth_env,
        headers: None,
    };

    // Calculate hints from catalog
    let hints = if let Some(model) = &catalog_model {
        let est_cost_micro = model
            .cost
            .input_per_million_micro
            .zip(model.cost.output_per_million_micro)
            .map(|(input_cost, output_cost)| {
                let prompt_tokens = route_req.estimates.prompt_tokens.unwrap_or(1000) as u64;
                let output_tokens = route_req.estimates.max_output_tokens.unwrap_or(500) as u64;
                (prompt_tokens * input_cost + output_tokens * output_cost) / 1_000_000
            });

        RouteHints {
            tier: model
                .policy_tags
                .iter()
                .find(|t| t.starts_with('T'))
                .cloned(),
            est_cost_micro,
            currency: Some(model.cost.currency.clone()),
            est_latency_ms: model.slos.recent.as_ref().and_then(|r| r.p50_ms),
            provider: Some(provider.to_string()),
            penalty: None,
        }
    } else {
        RouteHints::default()
    };

    // Add prompt overlay for educational contexts
    let prompt_overlays = if route_req.org.role.as_deref() == Some("student")
        || route_req.alias.starts_with("edu-")
    {
        let overlay_text =
            "You are Mesurii, a patient educational assistant. Provide hints, not full solutions."
                .to_string();
        Some(PromptOverlays {
            system_overlay: Some(overlay_text.clone()),
            overlay_fingerprint: Some("sha256:edu_overlay_v1".to_string()),
            overlay_size_bytes: Some(overlay_text.len() as u64),
            max_overlay_bytes: Some(16 * 1024),
        })
    } else {
        None
    };

    // Build limits
    let limits = RouteLimits {
        max_input_tokens: catalog_model
            .as_ref()
            .and_then(|m| m.capabilities.context_tokens),
        max_output_tokens: route_req.estimates.max_output_tokens,
        timeout_ms: route_req.targets.p95_latency_ms.or(Some(30000)),
    };

    // Generate plan
    let plan = RoutePlan {
        schema_version: Some("1.1".to_string()),
        route_id: state.next_route_id(),
        upstream,
        limits,
        prompt_overlays,
        hints,
        fallbacks: vec![],
        cache: Some(CacheControl {
            ttl_ms: 15000,
            etag: Some(format!(
                "\"{}@{}\"",
                state.catalog.revision,
                state.policy_revision()
            )),
            valid_until: None,
            freeze_key: Some(format!("freeze_{}", state.policy_revision())),
        }),
        policy_rev: Some(state.policy_revision()),
        policy: Some(PolicyInfo {
            revision: Some(state.policy_revision()),
            id: Some("example_policy_v1".to_string()),
            explain: Some("Sample decision for demo router".to_string()),
        }),
        stickiness: Some(Stickiness {
            plan_token: Some(format!(
                "plan_{}",
                route_req
                    .request_id
                    .clone()
                    .unwrap_or_else(|| "anon".to_string())
            )),
            max_turns: Some(3),
            expires_at: None,
        }),
        content_used: route_req
            .content_attestation
            .as_ref()
            .and_then(|c| c.included.clone())
            .or(Some("none".to_string())),
    };

    println!(
        "✅ Route plan: route_id={}, model={}, tier={:?}, est_cost_micro={:?}",
        plan.route_id, plan.upstream.model_id, plan.hints.tier, plan.hints.est_cost_micro
    );

    HttpResponse::Ok()
        .insert_header(("Router-Schema", "1.1"))
        .insert_header(("Config-Revision", state.policy_revision()))
        .insert_header(("X-Route-Id", plan.route_id.clone()))
        .insert_header(("X-Resolved-Model", plan.upstream.model_id.clone()))
        .insert_header((
            "X-Route-Tier",
            plan.hints
                .tier
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        ))
        .insert_header((
            "X-Policy-Rev",
            plan.policy
                .as_ref()
                .and_then(|p| p.revision.clone())
                .unwrap_or_else(|| "unknown".to_string()),
        ))
        .insert_header((
            "X-Content-Used",
            plan.content_used
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        ))
        .insert_header(("X-Route-Cache", "miss"))
        .insert_header((
            "Router-Latency",
            format!("{}ms", start.elapsed().as_millis()),
        ))
        .json(plan)
}

/// POST /route/feedback - Receive feedback
async fn route_feedback(
    state: web::Data<RouterState>,
    feedback: web::Json<RouteFeedback>,
) -> impl Responder {
    let fb = feedback.into_inner();

    println!(
        "📊 Feedback: route_id={}, success={}, duration={:?}ms, tokens={:?}, cost_micro={:?}, cache_hit={:?}",
        fb.route_id, fb.success, fb.duration_ms, fb.usage, fb.actual_cost_micro, fb.cache_hit
    );

    // Store feedback
    let mut log = state.feedback_log.lock().unwrap();
    log.push(fb);

    // Keep only last 1000 entries
    let log_len = log.len();
    if log_len > 1000 {
        log.drain(0..log_len - 1000);
    }

    HttpResponse::Ok().json(serde_json::json!({"status": "ok"}))
}

/// GET /health - Health check
async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "router",
        "version": "0.3.0"
    }))
}

/// GET /stats - Router statistics
async fn stats(state: web::Data<RouterState>) -> impl Responder {
    let feedback_log = state.feedback_log.lock().unwrap();
    let total_requests = feedback_log.len();
    let successful = feedback_log.iter().filter(|f| f.success).count();

    let avg_duration = if !feedback_log.is_empty() {
        feedback_log
            .iter()
            .filter_map(|f| f.duration_ms)
            .sum::<u64>() as f64
            / feedback_log.len() as f64
    } else {
        0.0
    };

    HttpResponse::Ok().json(serde_json::json!({
        "total_requests": total_requests,
        "successful_requests": successful,
        "failed_requests": total_requests - successful,
        "avg_duration_ms": avg_duration,
        "catalog_revision": state.catalog.revision,
        "policy_revision": state.policy_revision(),
        "model_count": state.catalog.models.len()
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("🚀 Router Service v0.3 starting...");

    let state = web::Data::new(RouterState::new());

    println!("📚 Loaded {} models in catalog", state.catalog.models.len());
    println!("🔧 Policy revision: {}", state.policy_revision());
    println!("🌐 Listening on http://0.0.0.0:9090");
    println!();
    println!("Endpoints:");
    println!("  GET  /catalog/models  - Model catalog");
    println!("  POST /route/plan      - Routing decision");
    println!("  POST /route/feedback  - Feedback collection");
    println!("  GET  /health          - Health check");
    println!("  GET  /stats           - Statistics");
    println!();
    println!("Test with:");
    println!("  curl http://localhost:9090/catalog/models | jq");
    println!("  CHAT2R_ROUTER_URL=http://localhost:9090 chat2response");
    println!();

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/catalog/models", web::get().to(get_catalog))
            .route("/route/plan", web::post().to(route_plan))
            .route("/route/feedback", web::post().to(route_feedback))
            .route("/health", web::get().to(health))
            .route("/stats", web::get().to(stats))
    })
    .bind("0.0.0.0:9090")?
    .run()
    .await
}
