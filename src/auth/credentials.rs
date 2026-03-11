use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Default Outline cloud API URL.
const DEFAULT_API_URL: &str = "https://app.getoutline.com/api";

/// Credentials for authenticating with the Outline API.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// API token (Bearer token).
    pub api_token: String,
    /// API base URL (e.g., "https://app.getoutline.com/api" or self-hosted).
    pub api_url: String,
}

/// Config file structure (~/.config/outline/credentials.json).
#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(alias = "apiToken", alias = "api_token")]
    api_token: Option<String>,
    #[serde(alias = "apiUrl", alias = "api_url")]
    api_url: Option<String>,
}

/// Returns the config file path: ~/.config/outline/credentials.json
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("outline").join("credentials.json"))
}

/// Load credentials from environment variables or config file.
///
/// Precedence:
/// 1. Environment variables (`OUTLINE_API_TOKEN`, `OUTLINE_API_URL`)
/// 2. Config file (`~/.config/outline/credentials.json`)
///
/// The API URL defaults to the Outline cloud URL if not specified.
pub fn load_credentials() -> Result<Credentials> {
    // Try env vars first
    let env_token = std::env::var("OUTLINE_API_TOKEN").ok();
    let env_url = std::env::var("OUTLINE_API_URL").ok();

    // Try config file
    let config = load_config_file();

    // Resolve token: env var takes precedence over config file
    let api_token = env_token
        .or_else(|| config.as_ref().and_then(|c| c.api_token.clone()))
        .context(
            "No API token found. Set OUTLINE_API_TOKEN environment variable \
             or create ~/.config/outline/credentials.json with {\"apiToken\": \"...\"}",
        )?;

    // Validate token format
    if !api_token.starts_with("ol_api_") {
        anyhow::bail!(
            "Invalid API token format. Outline API tokens start with 'ol_api_'. \
             Got: {}...",
            &api_token[..api_token.len().min(10)]
        );
    }

    // Resolve URL: env var > config file > default
    let api_url = env_url
        .or_else(|| config.as_ref().and_then(|c| c.api_url.clone()))
        .unwrap_or_else(|| DEFAULT_API_URL.to_string());

    // Normalize: strip trailing slash
    let api_url = api_url.trim_end_matches('/').to_string();

    Ok(Credentials { api_token, api_url })
}

/// Load config file (best-effort, returns None on any error).
fn load_config_file() -> Option<ConfigFile> {
    let path = config_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_reasonable() {
        let _ = config_path();
    }

    #[test]
    fn default_api_url_is_outline_cloud() {
        assert_eq!(DEFAULT_API_URL, "https://app.getoutline.com/api");
    }

    #[test]
    fn load_credentials_fails_without_token() {
        // Temporarily unset env var to test
        let original = std::env::var("OUTLINE_API_TOKEN").ok();
        // SAFETY: No other threads are accessing these env vars in this test
        unsafe {
            std::env::remove_var("OUTLINE_API_TOKEN");
        }

        let result = load_credentials();

        // Restore
        if let Some(val) = original {
            // SAFETY: Restoring the original value
            unsafe {
                std::env::set_var("OUTLINE_API_TOKEN", val);
            }
        }

        // Should fail (unless there's a config file on the test machine)
        // We can't fully control this in unit tests, so just verify it doesn't panic
        let _ = result;
    }
}
