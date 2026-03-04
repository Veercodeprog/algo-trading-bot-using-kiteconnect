// src/account.rs
use anyhow::{Result, anyhow};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn flatten_equity_margins(margins: &Value) -> Result<BTreeMap<String, Value>> {
    // margins is typically { "equity": {...}, "commodity": {...} } [web:81]
    let equity = margins
        .get("equity")
        .ok_or_else(|| anyhow!("margins response missing 'equity'"))?;

    let mut flat = BTreeMap::<String, Value>::new();

    // Copy top-level fields
    for key in ["enabled", "net"] {
        if let Some(v) = equity.get(key) {
            flat.insert(key.to_string(), v.clone());
        }
    }

    // Flatten equity.available.* as available_<key> [web:81]
    if let Some(obj) = equity.get("available").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            flat.insert(format!("available_{k}"), v.clone());
        }
    }

    // Flatten equity.utilised.* as utilised_<key> [web:81]
    if let Some(obj) = equity.get("utilised").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            flat.insert(format!("utilised_{k}"), v.clone());
        }
    }

    Ok(flat)
}

pub fn print_flat_row(row: &BTreeMap<String, Value>) {
    // “DataFrame-like” single-row display for CLI
    for (k, v) in row {
        println!("{:<28} {}", k, v);
    }
}
