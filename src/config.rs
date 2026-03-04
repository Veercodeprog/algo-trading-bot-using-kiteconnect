use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub api_secret: String,
    pub redirect_url: String,
    pub listen_addr: String,
    pub token_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("KITE_API_KEY").context("KITE_API_KEY missing")?;
        let api_secret = std::env::var("KITE_API_SECRET").context("KITE_API_SECRET missing")?;
        let redirect_url = std::env::var("KITE_REDIRECT_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:7000/callback".to_string());
        let listen_addr =
            std::env::var("KITE_LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:7000".to_string());
        let token_path =
            std::env::var("KITE_TOKEN_PATH").unwrap_or_else(|_| "token.json".to_string());

        Ok(Self {
            api_key,
            api_secret,
            redirect_url,
            listen_addr,
            token_path,
        })
    }
}
