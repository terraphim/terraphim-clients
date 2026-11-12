//! The config plane's write contract: `If-Match` and the bearer key (#459).
//!
//! The server protects its configuration with optimistic concurrency: a write
//! carries the `ETag` of the configuration it is based on, and is rejected
//! when that configuration has changed since. The client therefore has to
//!
//! 1. read the configuration (and its ETag) before every write,
//! 2. send the ETag back as `If-Match`,
//! 3. report a conflict instead of retrying a write that was computed against
//!    a configuration that no longer exists, and
//! 4. authenticate with the bearer key when one is configured.
//!
//! The tests drive the real client against a mock server, so the assertions
//! are on the bytes that go over the wire.

use serde_json::{Value, json};
use terraphim_agent::client::ApiClient;
use terraphim_config::{Config, Role};
use terraphim_types::RoleName;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The ETag the mock server hands out for the current configuration.
const CURRENT_ETAG: &str = "\"cfg-2\"";

/// The ETag of a configuration that has been changed since.
const STALE_ETAG: &str = "\"cfg-1\"";

/// A configuration with one role, whose API key is the redaction marker the
/// server hands out.
fn sample_config() -> Config {
    let role = Role {
        name: RoleName::new("Terraphim Engineer"),
        shortname: Some("TerraEng".to_string()),
        llm_api_key: Some("[REDACTED]".to_string()),
        ..Role::default()
    };

    let mut config = Config::default();
    config
        .roles
        .insert(RoleName::new("Terraphim Engineer"), role);
    config.default_role = RoleName::new("Terraphim Engineer");
    config.selected_role = RoleName::new("Terraphim Engineer");
    config
}

/// The body the server answers a config read with.
fn config_body(config: &Config) -> Value {
    json!({ "status": "ok", "config": config })
}

/// Serve `GET /config` with an ETag.
async fn serve_config(server: &MockServer, etag: &str, config: &Config) {
    Mock::given(method("GET"))
        .and(path("/config"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", etag)
                .set_body_json(config_body(config)),
        )
        .mount(server)
        .await;
}

/// The header values the mock server saw for a request.
fn headers_of(request: &wiremock::Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// A write carries the ETag of the configuration it is based on.
#[tokio::test]
async fn a_write_carries_the_etag_as_if_match() {
    let server = MockServer::start().await;
    let config = sample_config();
    serve_config(&server, CURRENT_ETAG, &config).await;

    // The write only matches when the client sends the ETag.
    Mock::given(method("POST"))
        .and(path("/config/selected_role"))
        .and(header("if-match", CURRENT_ETAG))
        .respond_with(ResponseTemplate::new(200).set_body_json(config_body(&config)))
        .expect(1)
        .mount(&server)
        .await;

    let api = ApiClient::new(server.uri());
    let snapshot = api
        .get_config_snapshot()
        .await
        .expect("the configuration must be readable");
    assert_eq!(
        snapshot.etag.as_deref(),
        Some(CURRENT_ETAG),
        "the ETag of the response must be available for the write"
    );

    api.update_selected_role("Terraphim Engineer", snapshot.etag.as_deref())
        .await
        .expect("the write must succeed");

    let requests = server
        .received_requests()
        .await
        .expect("the mock server records the requests it received");
    let write = requests
        .iter()
        .find(|request| request.url.path() == "/config/selected_role")
        .expect("the write must have been sent");
    assert_eq!(
        headers_of(write, "if-match").as_deref(),
        Some(CURRENT_ETAG),
        "the write must carry the ETag as If-Match"
    );
}

/// The configured key is sent as a bearer token.
#[tokio::test]
async fn the_api_key_is_sent_as_a_bearer_token() {
    let server = MockServer::start().await;
    let config = sample_config();
    serve_config(&server, CURRENT_ETAG, &config).await;

    let api = ApiClient::with_api_key(server.uri(), "s3cret-key");
    api.get_config_snapshot()
        .await
        .expect("the configuration must be readable");

    let requests = server
        .received_requests()
        .await
        .expect("the mock server records the requests it received");
    let read = requests
        .iter()
        .find(|request| request.url.path() == "/config")
        .expect("the read must have been sent");
    assert_eq!(
        headers_of(read, "authorization").as_deref(),
        Some("Bearer s3cret-key"),
        "the key must be sent as a bearer token"
    );
}

/// A write based on a configuration that changed is reported, not retried.
#[tokio::test]
async fn a_stale_etag_is_reported_as_a_conflict() {
    let server = MockServer::start().await;
    let config = sample_config();
    serve_config(&server, CURRENT_ETAG, &config).await;

    Mock::given(method("POST"))
        .and(path("/config/selected_role"))
        .and(header("if-match", STALE_ETAG))
        .respond_with(ResponseTemplate::new(412))
        .mount(&server)
        .await;

    let api = ApiClient::new(server.uri());
    // Read the configuration first, the way every write path does.
    api.get_config_snapshot()
        .await
        .expect("the configuration must be readable");

    let error = api
        .update_selected_role("Terraphim Engineer", Some(STALE_ETAG))
        .await
        .expect_err("a stale ETag must fail the write");

    let message = error.to_string();
    assert!(
        message.contains("changed elsewhere"),
        "the conflict must be reported plainly, got: {message}"
    );

    let requests = server
        .received_requests()
        .await
        .expect("the mock server records the requests it received");
    let writes = requests
        .iter()
        .filter(|request| request.url.path() == "/config/selected_role")
        .count();
    assert_eq!(
        writes, 1,
        "the write must not be retried behind the user's back"
    );
    let reads = requests
        .iter()
        .filter(|request| request.url.path() == "/config")
        .count();
    assert!(
        reads >= 2,
        "the configuration must be fetched again to report the conflict, got {reads} reads"
    );
}

/// A server that demands `If-Match` but sends no ETag is reported as a
/// mismatch rather than as a generic write failure.
#[tokio::test]
async fn a_server_that_demands_if_match_without_an_etag_is_reported() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/config/selected_role"))
        .respond_with(ResponseTemplate::new(428))
        .mount(&server)
        .await;

    let api = ApiClient::new(server.uri());
    let error = api
        .update_selected_role("Terraphim Engineer", None)
        .await
        .expect_err("a write without an ETag must fail");

    let message = error.to_string();
    assert!(
        message.contains("If-Match"),
        "the error must name the missing precondition, got: {message}"
    );
}

/// The values the server redacted are sent back unchanged, so the server can
/// keep the values it holds.
#[tokio::test]
async fn redacted_values_are_sent_back_unchanged() {
    let server = MockServer::start().await;
    let config = sample_config();
    serve_config(&server, CURRENT_ETAG, &config).await;

    Mock::given(method("POST"))
        .and(path("/config"))
        .and(header("if-match", CURRENT_ETAG))
        .respond_with(ResponseTemplate::new(200).set_body_json(config_body(&config)))
        .expect(1)
        .mount(&server)
        .await;

    let api = ApiClient::new(server.uri());
    let snapshot = api
        .get_config_snapshot()
        .await
        .expect("the configuration must be readable");

    let mut updated = snapshot.config;
    updated.selected_role = RoleName::new("Terraphim Engineer");
    api.post_config(&updated, snapshot.etag.as_deref())
        .await
        .expect("the write must succeed");

    let requests = server
        .received_requests()
        .await
        .expect("the mock server records the requests it received");
    let write = requests
        .iter()
        .find(|request| request.url.path() == "/config" && request.method == reqwest::Method::POST)
        .expect("the write must have been sent");

    let body: Value = serde_json::from_slice(&write.body).expect("the write body is JSON");
    let api_key = body["roles"]["Terraphim Engineer"]["llm_api_key"].as_str();
    assert_eq!(
        api_key,
        Some("[REDACTED]"),
        "the redacted value must be sent back unchanged, got: {body}"
    );
}

/// The TUI's role switch goes through the same read-then-write path, so it
/// carries the ETag too.
#[tokio::test]
async fn the_tui_role_switch_carries_the_etag() {
    use terraphim_agent::tui_backend::TuiBackend;

    let server = MockServer::start().await;
    let config = sample_config();
    serve_config(&server, CURRENT_ETAG, &config).await;

    Mock::given(method("POST"))
        .and(path("/config/selected_role"))
        .and(header("if-match", CURRENT_ETAG))
        .respond_with(ResponseTemplate::new(200).set_body_json(config_body(&config)))
        .expect(1)
        .mount(&server)
        .await;

    let backend = TuiBackend::Remote(ApiClient::new(server.uri()));
    backend
        .switch_role("Terraphim Engineer")
        .await
        .expect("the role switch must succeed");

    let requests = server
        .received_requests()
        .await
        .expect("the mock server records the requests it received");
    let write = requests
        .iter()
        .find(|request| request.url.path() == "/config/selected_role")
        .expect("the role switch must have been sent");
    assert_eq!(
        headers_of(write, "if-match").as_deref(),
        Some(CURRENT_ETAG),
        "the role switch must carry the ETag as If-Match"
    );
}
