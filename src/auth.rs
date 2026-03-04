use crate::token_store::save_token;
use anyhow::{Context, Result, anyhow};
use axum::{Router, extract::Query, response::Html, routing::get};
use serde::Deserialize;
use tokio::sync::oneshot;

use kiteconnect::connect::KiteConnect;

use serde_json::Value;

use crate::{
    broker::Broker,
    config::Config,
    token_store::{StoredToken, load_token},
};

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    request_token: Option<String>,
}

pub async fn login_and_store_token(cfg: &Config) -> Result<StoredToken> {
    let (tx, rx) = oneshot::channel::<String>();
    let tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx)));

    let app = Router::new().route(
        "/callback",
        get({
            let tx = tx.clone();
            move |Query(q): Query<CallbackQuery>| {
                let tx = tx.clone();
                async move {
                    if let Some(rt) = q.request_token {
                        if let Some(sender) = tx.lock().await.take() {
                            let _ = sender.send(rt);
                        }
                        Html("Login successful. You can close this tab.")
                    } else {
                        Html("Missing request_token in callback URL.")
                    }
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr)
        .await
        .with_context(|| format!("bind {}", cfg.listen_addr))?;

    // Create kite client (empty access token initially)
    let mut kite = KiteConnect::new(&cfg.api_key, "");

    // IMPORTANT: your app's redirect URL in Zerodha console must match cfg.redirect_url
    // The login URL will redirect there with ?request_token=... on success.
    let login_url = kite.login_url();
    webbrowser::open(&login_url).context("failed to open browser")?;

    let server = axum::serve(listener, app);
    let server_handle = tokio::spawn(async move {
        let _ = server.await;
    });

    let request_token = tokio::time::timeout(std::time::Duration::from_secs(180), rx)
        .await
        .context("timeout waiting for request_token")?
        .context("callback channel closed")?;

    // Exchange request_token for access_token (generate_session also sets token internally in this crate)
    let resp = kite
        .generate_session(&request_token, &cfg.api_secret)
        .map_err(|e| anyhow!("generate_session failed: {:?}", e))?;

    let access_token = resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("access_token missing in generate_session response"))?
        .to_string();

    let stored = StoredToken {
        access_token,
        created_at_unix: chrono_unix_now(),
    };

    save_token(&cfg.token_path, &stored)?;
    server_handle.abort(); // stop server once we got the token
    Ok(stored)
}
pub async fn ensure_token(cfg: &Config) -> Result<StoredToken> {
    if let Some(tok) = load_token(&cfg.token_path)? {
        let mut broker = Broker::new(&cfg.api_key, &tok.access_token);
        let ok: Result<Value> = broker.profile();
        if ok.is_ok() {
            return Ok(tok);
        }
    }

    // Fresh login flow (browser opens, callback captures request_token, token saved)
    login_and_store_token(cfg).await
}
fn chrono_unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
