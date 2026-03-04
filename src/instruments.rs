use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde::Deserialize;
use std::{collections::BTreeSet, fs, io::Read, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct Instrument {
    pub instrument_token: u64,
    pub exchange_token: u64,
    pub tradingsymbol: String,
    pub name: String,

    #[serde(deserialize_with = "de_opt_f64")]
    pub last_price: Option<f64>,

    #[serde(deserialize_with = "de_opt_date")]
    pub expiry: Option<NaiveDate>,

    #[serde(deserialize_with = "de_opt_f64")]
    pub strike: Option<f64>,

    #[serde(deserialize_with = "de_f64")]
    pub tick_size: f64,

    #[serde(deserialize_with = "de_u32")]
    pub lot_size: u32,

    pub instrument_type: String,
    pub segment: String,
    pub exchange: String,
}

pub async fn fetch_instruments_csv(
    api_key: &str,
    access_token: &str,
    exchange: Option<&str>,
) -> Result<Vec<u8>> {
    // GET /instruments or /instruments/:exchange [web:96]
    let url = match exchange {
        Some(ex) => format!("https://api.kite.trade/instruments/{ex}"),
        None => "https://api.kite.trade/instruments".to_string(),
    };

    // Auth header format in docs: "Authorization: token api_key:access_token" [web:96]
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("X-Kite-Version", "3")
        .header("Authorization", format!("token {api_key}:{access_token}"))
        .send()
        .await
        .context("instruments request failed")?;

    let status = resp.status();
    if !status.is_success() {
        bail!("instruments download failed (HTTP {status})");
    }

    Ok(resp.bytes().await?.to_vec())
}

pub fn maybe_gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    // If server already decoded gzip, bytes will be plain CSV.
    // If it is gzipped, first two bytes are 0x1f 0x8b.
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut gz = flate2::read::GzDecoder::new(bytes);
        let mut out = Vec::new();
        gz.read_to_end(&mut out).context("gunzip instruments")?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

pub fn parse_instruments(csv_bytes: &[u8]) -> Result<Vec<Instrument>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_bytes);

    let mut out = Vec::new();
    for rec in rdr.deserialize() {
        let inst: Instrument = rec.context("deserialize instrument row")?;
        out.push(inst);
    }
    Ok(out)
}

pub async fn load_or_download(
    api_key: &str,
    access_token: &str,
    cache_path: &str,
) -> Result<Vec<Instrument>> {
    // The dump is generated once everyday, so caching is reasonable. [web:96]
    if Path::new(cache_path).exists() {
        let raw = fs::read(cache_path).context("read instruments cache")?;
        let csv = maybe_gunzip(&raw)?;
        return parse_instruments(&csv);
    }

    let raw = fetch_instruments_csv(api_key, access_token, None).await?;
    fs::write(cache_path, &raw).context("write instruments cache")?;
    let csv = maybe_gunzip(&raw)?;
    parse_instruments(&csv)
}

pub fn demo_filters(instruments: &[Instrument]) {
    println!(
        "Successfully downloaded and processed {} instruments.",
        instruments.len()
    );

    let exchanges: BTreeSet<&str> = instruments.iter().map(|x| x.exchange.as_str()).collect();
    println!("Available Exchanges: {:?}", exchanges);

    let nse: Vec<&Instrument> = instruments
        .iter()
        .filter(|x| x.exchange == "NSE" && x.segment == "NSE")
        .take(5)
        .collect();
    println!("\nExample NSE equities (first 5):");
    for x in nse {
        println!("{} | {} | {}", x.exchange, x.tradingsymbol, x.name);
    }

    let mut nifty_ce: Vec<&Instrument> = instruments
        .iter()
        .filter(|x| x.exchange == "NFO" && x.name == "NIFTY" && x.instrument_type == "CE")
        .collect();

    nifty_ce.sort_by_key(|x| x.expiry);
    println!("\nExample NIFTY CE options (sorted by expiry, first 5):");
    for x in nifty_ce.into_iter().take(5) {
        println!(
            "{} | {} | expiry={:?} | strike={:?}",
            x.exchange, x.tradingsymbol, x.expiry, x.strike
        );
    }

    let mut gold_fut: Vec<&Instrument> = instruments
        .iter()
        .filter(|x| x.exchange == "MCX" && x.name == "GOLD" && x.segment == "MCX-FUT")
        .collect();

    gold_fut.sort_by_key(|x| x.expiry);
    println!("\nExample GOLD futures (sorted by expiry, first 5):");
    for x in gold_fut.into_iter().take(5) {
        println!(
            "{} | {} | expiry={:?}",
            x.exchange, x.tradingsymbol, x.expiry
        );
    }
}

/* ---- serde helpers ---- */
fn de_opt_f64<'de, D>(d: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<f64>().map(Some).map_err(serde::de::Error::custom)
}

fn de_f64<'de, D>(d: D) -> std::result::Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    s.trim().parse::<f64>().map_err(serde::de::Error::custom)
}

fn de_u32<'de, D>(d: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    s.trim().parse::<u32>().map_err(serde::de::Error::custom)
}

fn de_opt_date<'de, D>(d: D) -> std::result::Result<Option<NaiveDate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(d)?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(Some)
        .map_err(serde::de::Error::custom)
}
// src/instruments.rs
pub fn find_instrument_token(
    instruments: &[Instrument],
    exchange: &str,
    tradingsymbol: &str,
) -> Option<u64> {
    instruments
        .iter()
        .find(|x| x.exchange == exchange && x.tradingsymbol == tradingsymbol)
        .map(|x| x.instrument_token)
}
