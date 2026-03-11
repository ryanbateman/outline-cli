use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Embedded spec compiled into the binary as offline fallback.
const EMBEDDED_SPEC: &str = include_str!("../../api/spec3.json");

/// Remote URL for fetching the latest spec.
const SPEC_URL: &str = "https://raw.githubusercontent.com/outline/openapi/main/spec3.json";

/// Cache TTL: 24 hours.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Returns the cache file path: ~/.cache/outline/spec3.json
fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("outline").join("spec3.json"))
}

/// Load the OpenAPI spec JSON string.
///
/// Strategy:
/// 1. If a valid (< 24h old) cache file exists, use it.
/// 2. Otherwise, try to fetch from GitHub.
/// 3. If fetch fails, fall back to the embedded spec.
pub async fn load_spec() -> Result<String> {
    // 1. Check cache
    if let Some(cached) = read_cache() {
        return Ok(cached);
    }

    // 2. Try fetch
    match fetch_spec().await {
        Ok(spec) => {
            // Write cache (best-effort, don't fail if we can't)
            let _ = write_cache(&spec);
            Ok(spec)
        }
        Err(e) => {
            eprintln!("Warning: Failed to fetch spec from GitHub: {e}. Using embedded spec.");
            Ok(EMBEDDED_SPEC.to_string())
        }
    }
}

/// Load the spec synchronously using only cache or embedded fallback.
/// Used in contexts where async is inconvenient (e.g., tests, MCP mode).
#[allow(dead_code)]
pub fn load_spec_sync() -> String {
    if let Some(cached) = read_cache() {
        return cached;
    }
    EMBEDDED_SPEC.to_string()
}

/// Read from cache if it exists and is fresh (< TTL).
fn read_cache() -> Option<String> {
    let path = cache_path()?;
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;

    if age > CACHE_TTL {
        return None;
    }

    std::fs::read_to_string(&path).ok()
}

/// Write spec to cache file.
fn write_cache(spec: &str) -> Result<()> {
    let path = cache_path().context("Could not determine cache directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }
    std::fs::write(&path, spec).context("Failed to write cache file")?;
    Ok(())
}

/// Fetch spec from GitHub.
async fn fetch_spec() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("Failed to build HTTP client")?;

    let resp = client
        .get(SPEC_URL)
        .send()
        .await
        .context("Failed to fetch spec from GitHub")?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("GitHub returned HTTP {status}");
    }

    resp.text()
        .await
        .context("Failed to read spec response body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_spec_is_valid_json() {
        let v: serde_json::Value =
            serde_json::from_str(EMBEDDED_SPEC).expect("embedded spec should be valid JSON");
        assert_eq!(v["openapi"], "3.0.0");
    }

    #[test]
    fn embedded_spec_has_paths() {
        let v: serde_json::Value = serde_json::from_str(EMBEDDED_SPEC).unwrap();
        let paths = v["paths"].as_object().expect("paths should be an object");
        assert!(paths.len() > 50, "spec should have many paths");
    }

    #[test]
    fn cache_path_is_reasonable() {
        // Just verify it doesn't panic; actual path depends on platform
        let _ = cache_path();
    }
}
