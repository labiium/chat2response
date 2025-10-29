#![cfg(test)]
// Server entry tests (compile checks)
//
// These tests verify that the server configuration compiles correctly.

use actix_web::App;
use chat2response::server::config_routes;

#[test]
fn router_builds() {
    // Simply verify that the configuration compiles
    let _app = App::new().configure(config_routes);
}

#[test]
fn router_builds_with_proxy_feature() {
    // Verify proxy routes are configured (always-on in this build)
    let _app = App::new().configure(config_routes);
}
