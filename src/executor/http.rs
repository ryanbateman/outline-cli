use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use serde_json::Value;
use std::time::Duration;

use crate::auth::Credentials;

/// Maximum number of retry attempts for retryable errors (429/5xx).
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (doubles each retry).
const BASE_DELAY_MS: u64 = 500;

/// Maximum backoff delay cap.
const MAX_DELAY_MS: u64 = 30_000;

/// Build default headers for Outline API requests.
fn build_headers(credentials: &Credentials) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", credentials.api_token))
            .context("Invalid API token for header")?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&format!("outline-cli/{}", env!("CARGO_PKG_VERSION")))
            .unwrap_or_else(|_| HeaderValue::from_static("outline-cli")),
    );
    Ok(headers)
}

/// Build a configured reqwest client with standard headers.
fn build_client(credentials: &Credentials) -> Result<reqwest::Client> {
    let headers = build_headers(credentials)?;
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .default_headers(headers)
        .build()
        .context("Failed to build HTTP client")
}

/// Determine if an HTTP status code is retryable.
fn is_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Parse Retry-After header value (seconds).
/// Supports integer seconds format. Ignores HTTP-date format for simplicity.
fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Calculate backoff delay for a given retry attempt.
/// Uses exponential backoff: base * 2^attempt, capped at MAX_DELAY_MS.
fn backoff_delay(attempt: u32) -> Duration {
    let delay = BASE_DELAY_MS.saturating_mul(1u64 << attempt);
    Duration::from_millis(delay.min(MAX_DELAY_MS))
}

/// Response from the executor, including status code for error handling.
pub struct ApiResponse {
    pub body: Value,
    pub status: u16,
}

impl ApiResponse {
    /// Whether the HTTP request succeeded (2xx).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Execute a single API request with retry logic.
///
/// All Outline API methods are POST requests to `{base_url}/{path}`.
/// Retries on 429 (rate limit) and 5xx (server error) with exponential backoff.
pub async fn execute_request(
    credentials: &Credentials,
    path: &str,
    body: Option<&Value>,
) -> Result<ApiResponse> {
    let client = build_client(credentials)?;
    let url = format!("{}{}", credentials.api_url, path);
    let payload = body.cloned().unwrap_or_else(|| serde_json::json!({}));

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..=MAX_RETRIES {
        let resp = client.post(&url).json(&payload).send().await;

        match resp {
            Ok(response) => {
                let status = response.status();

                if is_retryable(status) && attempt < MAX_RETRIES {
                    // Use Retry-After header if present, otherwise exponential backoff
                    let delay =
                        parse_retry_after(&response).unwrap_or_else(|| backoff_delay(attempt));
                    eprintln!(
                        "Retryable error ({}), retrying in {}ms (attempt {}/{})",
                        status.as_u16(),
                        delay.as_millis(),
                        attempt + 1,
                        MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                let status_code = status.as_u16();
                let response_body: Value = response
                    .json()
                    .await
                    .context("Failed to parse response as JSON")?;

                return Ok(ApiResponse {
                    body: response_body,
                    status: status_code,
                });
            }
            Err(e) => {
                if attempt < MAX_RETRIES {
                    let delay = backoff_delay(attempt);
                    eprintln!(
                        "Request failed ({}), retrying in {}ms (attempt {}/{})",
                        e,
                        delay.as_millis(),
                        attempt + 1,
                        MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                    last_error = Some(e.into());
                    continue;
                }
                return Err(e).context("Failed to send request after retries");
            }
        }
    }

    // Should not reach here, but handle gracefully
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Request failed after max retries")))
}

/// Execute a paginated API request, streaming all pages as NDJSON.
///
/// Sends the initial request, then follows pagination by incrementing offset
/// until no more pages exist. Each page's `data` items are printed as
/// individual JSON lines (NDJSON format).
///
/// Returns the total number of items streamed.
pub async fn execute_paginated(
    credentials: &Credentials,
    path: &str,
    body: Option<&Value>,
    fields: Option<&str>,
) -> Result<usize> {
    let mut current_body = body.cloned().unwrap_or_else(|| serde_json::json!({}));
    let mut total_items = 0;
    let mut offset: u64 = 0;

    // Get limit from body or default to 25
    let limit = current_body
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(25);

    loop {
        // Set offset in the body for each page
        current_body["offset"] = serde_json::json!(offset);
        if current_body.get("limit").is_none() {
            current_body["limit"] = serde_json::json!(limit);
        }

        let response = execute_request(credentials, path, Some(&current_body)).await?;

        if !response.is_success() || response.body.get("ok") == Some(&Value::Bool(false)) {
            // Print error and stop
            println!(
                "{}",
                serde_json::to_string(&response.body).unwrap_or_default()
            );
            return Ok(total_items);
        }

        // Extract and print data items
        let items = match response.body.get("data") {
            Some(Value::Array(arr)) => arr.clone(),
            _ => break,
        };

        if items.is_empty() {
            break;
        }

        for item in &items {
            let output = if let Some(field_list) = fields {
                filter_item(item, field_list)
            } else {
                item.clone()
            };
            println!("{}", serde_json::to_string(&output).unwrap_or_default());
        }

        total_items += items.len();

        // Check if there are more pages
        let has_more = response
            .body
            .get("pagination")
            .and_then(|p| p.get("nextPath"))
            .is_some_and(|v| !v.is_null());

        if !has_more {
            break;
        }

        offset += limit;
    }

    Ok(total_items)
}

/// Apply field filtering to a single item for NDJSON output.
fn filter_item(item: &Value, field_list: &str) -> Value {
    if let Value::Object(obj) = item {
        let fields: Vec<&str> = field_list.split(',').map(|f| f.trim()).collect();
        let filtered = crate::output::filter_object_fields(obj, &fields);
        Value::Object(filtered)
    } else {
        item.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_construction() {
        let base = "https://app.getoutline.com/api";
        let path = "/documents.create";
        let url = format!("{base}{path}");
        assert_eq!(url, "https://app.getoutline.com/api/documents.create");
    }

    #[test]
    fn backoff_increases_exponentially() {
        let d0 = backoff_delay(0);
        let d1 = backoff_delay(1);
        let d2 = backoff_delay(2);
        assert_eq!(d0.as_millis(), 500);
        assert_eq!(d1.as_millis(), 1000);
        assert_eq!(d2.as_millis(), 2000);
    }

    #[test]
    fn backoff_caps_at_max() {
        let d10 = backoff_delay(10);
        assert!(d10.as_millis() <= MAX_DELAY_MS as u128);
    }

    #[test]
    fn retryable_status_codes() {
        assert!(is_retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(reqwest::StatusCode::BAD_GATEWAY));
        assert!(is_retryable(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_retryable(reqwest::StatusCode::GATEWAY_TIMEOUT));
        assert!(!is_retryable(reqwest::StatusCode::OK));
        assert!(!is_retryable(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_retryable(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_retryable(reqwest::StatusCode::FORBIDDEN));
    }

    #[test]
    fn api_response_success_check() {
        let ok = ApiResponse {
            body: serde_json::json!({"ok": true}),
            status: 200,
        };
        assert!(ok.is_success());

        let not_found = ApiResponse {
            body: serde_json::json!({"ok": false}),
            status: 404,
        };
        assert!(!not_found.is_success());

        let rate_limited = ApiResponse {
            body: serde_json::json!({"ok": false}),
            status: 429,
        };
        assert!(!rate_limited.is_success());
    }
}
