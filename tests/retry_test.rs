use outline_cli::auth::Credentials;
use outline_cli::executor::execute_request;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper to create credentials pointing at a wiremock server.
fn mock_credentials(server: &MockServer) -> Credentials {
    Credentials {
        api_token: "ol_api_testtoken1234567890abcdefghijklmnop".to_string(),
        api_url: server.uri(),
    }
}

#[tokio::test]
async fn retries_on_429_then_succeeds() {
    let server = MockServer::start().await;
    let call_count = AtomicU32::new(0);

    // First 2 calls return 429, third returns 200
    Mock::given(method("POST"))
        .and(path("/documents.list"))
        .respond_with(move |_req: &wiremock::Request| {
            let count = call_count.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                ResponseTemplate::new(429).set_body_json(json!({
                    "ok": false,
                    "error": "rate_limit_exceeded",
                    "message": "Rate limit exceeded"
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "ok": true,
                    "data": [{"id": "abc", "title": "Test"}]
                }))
            }
        })
        .expect(3)
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(&creds, "/documents.list", None).await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(resp.is_success());
    assert_eq!(resp.body["ok"], true);
}

#[tokio::test]
async fn retries_on_500_then_succeeds() {
    let server = MockServer::start().await;
    let call_count = AtomicU32::new(0);

    Mock::given(method("POST"))
        .and(path("/documents.info"))
        .respond_with(move |_req: &wiremock::Request| {
            let count = call_count.fetch_add(1, Ordering::SeqCst);
            if count < 1 {
                ResponseTemplate::new(500).set_body_json(json!({
                    "ok": false,
                    "error": "internal_error",
                    "message": "Internal server error"
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "ok": true,
                    "data": {"id": "abc", "title": "Test Doc"}
                }))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(&creds, "/documents.info", Some(&json!({"id": "abc"}))).await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(resp.is_success());
    assert_eq!(resp.body["data"]["title"], "Test Doc");
}

#[tokio::test]
async fn does_not_retry_on_400() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/documents.create"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "ok": false,
            "error": "validation_error",
            "message": "title: Required"
        })))
        .expect(1) // Should only be called once — no retry
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(&creds, "/documents.create", Some(&json!({}))).await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(!resp.is_success());
    assert_eq!(resp.status, 400);
}

#[tokio::test]
async fn does_not_retry_on_404() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/documents.info"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "ok": false,
            "error": "not_found",
            "message": "Document not found"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(
        &creds,
        "/documents.info",
        Some(&json!({"id": "nonexistent"})),
    )
    .await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(!resp.is_success());
    assert_eq!(resp.status, 404);
}

#[tokio::test]
async fn returns_last_error_after_max_retries() {
    let server = MockServer::start().await;

    // Always return 503
    Mock::given(method("POST"))
        .and(path("/documents.list"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "ok": false,
            "error": "service_unavailable",
            "message": "Service unavailable"
        })))
        .expect(4) // initial + 3 retries
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(&creds, "/documents.list", None).await;

    // After max retries, should return the 503 response (not an Err)
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(!resp.is_success());
    assert_eq!(resp.status, 503);
}

#[tokio::test]
async fn success_on_first_attempt_no_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/collections.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "data": [{"id": "col1", "name": "Plans"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let creds = mock_credentials(&server);
    let result = execute_request(&creds, "/collections.list", None).await;

    assert!(result.is_ok());
    let resp = result.unwrap();
    assert!(resp.is_success());
    assert_eq!(resp.body["data"][0]["name"], "Plans");
}
