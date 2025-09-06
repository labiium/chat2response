#![cfg(test)]
// Server entry tests (compile checks)
//
// These tests intentionally avoid spinning up a live HTTP server.
// They ensure that the server router builds and satisfies basic trait
// bounds, catching accidental compile regressions in handlers/routes.

use chat2response::server::build_router;

/// Helper to assert a type is Send at compile time.
fn assert_send<T: Send>(_: T) {}

/// Helper to assert a type is 'static at compile time.
fn assert_static<T: 'static>(_: &T) {}

#[test]
fn router_builds() {
    let app = build_router();
    // Ensure the router value compiles and can be moved across threads (Send).
    assert_send(app);
}

#[test]
fn router_static_lifetime() {
    let app = build_router();
    // Verify 'static bound (no non-static borrows captured in the router).
    assert_static(&app);
}

// Router builds with proxy route enabled (always-on in this build).
#[test]
fn router_builds_with_proxy_feature() {
    let app = build_router();
    assert_send(app);
}
