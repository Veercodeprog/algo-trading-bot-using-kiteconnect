// src/session.rs
use anyhow::{Context, Result, bail};
use serde_json::Value;
use url::Url;

pub async fn logout(api_key: &str, access_token: &str) -> Result<Value> {
    // Docs: DELETE /session/token?api_key=xxx&access_token=yyy with X-Kite-Version: 3 [web:7]
    let mut url =
        Url::parse("https://api.kite.trade/session/token").expect("static URL must be valid");

    url.query_pairs_mut()
        .append_pair("api_key", api_key)
        .append_pair("access_token", access_token);

    let client = reqwest::Client::new();
    let resp = client
        .delete(url)
        .header("X-Kite-Version", "3")
        .send()
        .await
        .context("logout request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("logout response not JSON")?;

    if !status.is_success() {
        bail!("logout failed (HTTP {status}): {body}");
    }

    Ok(body)
}
