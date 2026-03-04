use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub created_at_unix: i64,
}

pub fn load_token(path: &str) -> Result<Option<StoredToken>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read token file: {path}"))?;
    let tok: StoredToken = serde_json::from_str(&raw).context("parse token json")?;
    Ok(Some(tok))
}

pub fn save_token(path: &str, tok: &StoredToken) -> Result<()> {
    let raw = serde_json::to_string_pretty(tok).context("serialize token json")?;
    fs::write(path, raw).with_context(|| format!("write token file: {path}"))?;

    // Best-effort: restrict perms on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perm).ok();
    }

    Ok(())
}
