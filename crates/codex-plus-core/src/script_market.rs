use anyhow::{Context, bail};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::user_scripts::UserScriptManager;

pub const DEFAULT_MARKET_INDEX_URL: &str =
    "https://raw.githubusercontent.com/BigPizzaV3/CodexPlusPlusScriptMarket/main/index.json";
const MARKET_HOST: &str = "raw.githubusercontent.com";
const MARKET_PATH_PREFIX: &str = "/BigPizzaV3/CodexPlusPlusScriptMarket/main/";
const MAX_MARKET_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptMarketManifest {
    pub version: u64,
    pub updated_at: Option<String>,
    pub scripts: Vec<MarketScript>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketScript {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub homepage: String,
    pub script_url: String,
    #[serde(default)]
    pub sha256: String,
}

pub fn parse_market_manifest(raw: Value) -> anyhow::Result<ScriptMarketManifest> {
    let version = raw.get("version").and_then(Value::as_u64).unwrap_or(1);
    let updated_at = raw
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let scripts = raw
        .get("scripts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_market_script)
        .collect();

    Ok(ScriptMarketManifest {
        version,
        updated_at,
        scripts,
    })
}

pub async fn fetch_market_manifest(url: &str) -> anyhow::Result<ScriptMarketManifest> {
    let url = validate_market_url(url)?;
    let response = market_http_client()?
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to request script market index {url}"))?
        .error_for_status()
        .with_context(|| format!("script market index returned an error status {url}"))?;
    let bytes = read_limited_response(response).await?;
    let raw =
        serde_json::from_slice(&bytes).context("failed to decode script market index JSON")?;
    parse_market_manifest(raw)
}

pub async fn download_script(url: &str) -> anyhow::Result<Vec<u8>> {
    let url = validate_market_url(url)?;
    let response = market_http_client()?
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to request script {url}"))?
        .error_for_status()
        .with_context(|| format!("script download returned an error status {url}"))?;
    read_limited_response(response).await
}

fn market_http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if validate_market_url(attempt.url().as_str()).is_ok() {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()?)
}

fn validate_market_url(url: &str) -> anyhow::Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url).context("script market URL is invalid")?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(MARKET_HOST)
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.path().starts_with(MARKET_PATH_PREFIX)
    {
        bail!("script market URL is outside the trusted repository");
    }
    Ok(parsed)
}

async fn read_limited_response(response: reqwest::Response) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MARKET_RESPONSE_BYTES)
    {
        bail!("script market response exceeds the size limit");
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read script market response")?;
        let next_length = bytes.len().saturating_add(chunk.len());
        if next_length > MAX_MARKET_RESPONSE_BYTES as usize {
            bail!("script market response exceeds the size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub fn install_market_script_content(
    manager: &UserScriptManager,
    script: &MarketScript,
    content: &[u8],
) -> anyhow::Result<()> {
    verify_script_sha256(&script.sha256, content)?;
    let path = manager.user_script_path_for_market_id(&script.id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create user script directory {}",
                parent.display()
            )
        })?;
    }
    let previous_content = std::fs::read(&path).ok();
    crate::settings::atomic_write(&path, content)
        .with_context(|| format!("failed to write script {}", path.display()))?;
    if let Err(error) = manager.record_market_install(script) {
        match previous_content {
            Some(previous) => {
                let _ = crate::settings::atomic_write(&path, &previous);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        return Err(error);
    }
    Ok(())
}

pub async fn install_market_script(
    manager: &UserScriptManager,
    script: &MarketScript,
) -> anyhow::Result<()> {
    let content = download_script(&script.script_url).await?;
    install_market_script_content(manager, script, &content)
}

pub(crate) fn verify_script_sha256(expected: &str, content: &[u8]) -> anyhow::Result<()> {
    let expected = expected.trim();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("market script is missing a valid SHA-256 checksum");
    }
    let actual = format!("{:x}", Sha256::digest(content));
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("market script checksum verification failed");
    }
    Ok(())
}

fn parse_market_script(raw: Value) -> Option<MarketScript> {
    let id = required_string(&raw, "id")?;
    let name = required_string(&raw, "name")?;
    let version = required_string(&raw, "version")?;
    let script_url = required_string(&raw, "script_url")?;
    let sha256 = required_sha256(&raw)?;
    Some(MarketScript {
        id,
        name,
        description: optional_string(&raw, "description"),
        version,
        author: optional_string(&raw, "author"),
        tags: raw
            .get("tags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        homepage: optional_string(&raw, "homepage"),
        script_url,
        sha256,
    })
}

fn required_sha256(raw: &Value) -> Option<String> {
    let value = required_string(raw, "sha256")?;
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn required_string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_string(raw: &Value, key: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_urls_are_limited_to_the_trusted_repository() {
        assert!(validate_market_url(DEFAULT_MARKET_INDEX_URL).is_ok());
        assert!(validate_market_url("https://example.com/index.json").is_err());
        assert!(
            validate_market_url("https://raw.githubusercontent.com/Other/Repo/main/index.json")
                .is_err()
        );
    }

    #[test]
    fn script_checksum_is_required_and_verified() {
        let content = b"window.demo = true;";
        let checksum = format!("{:x}", Sha256::digest(content));
        assert!(verify_script_sha256(&checksum, content).is_ok());
        assert!(verify_script_sha256("", content).is_err());
        assert!(verify_script_sha256(&"0".repeat(64), content).is_err());
    }
}
