use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use csv::Writer;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;

const IST_SECS: i32 = 5 * 3600 + 30 * 60;

fn ist() -> FixedOffset {
    FixedOffset::east_opt(IST_SECS).unwrap()
}

#[derive(Debug, Clone)]
struct Candle {
    ts: DateTime<FixedOffset>,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    oi: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrategyKind {
    Sma,
    Rmi,
}

impl StrategyKind {
    fn from_env() -> Self {
        match env::var("BACKTEST_STRATEGY")
            .unwrap_or_else(|_| "sma".to_string())
            .to_lowercase()
            .as_str()
        {
            "rmi" => StrategyKind::Rmi,
            _ => StrategyKind::Sma,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            StrategyKind::Sma => "sma",
            StrategyKind::Rmi => "rmi",
        }
    }
}

#[derive(Debug, Clone)]
struct Config {
    instrument_token: u32,
    symbol: String,
    exchange: String,
    interval: String,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,

    strategy: StrategyKind,

    fast_sma: usize,
    slow_sma: usize,

    rmi_length: usize,
    rmi_momentum: usize,
    rmi_buy_level: f64,
    rmi_sell_level: f64,
    enable_rmi_exit: bool,

    start_capital: f64,
    commission_pct: f64,
    slippage_pct: f64,

    enable_sma_exit: bool,
    enable_stop_loss: bool,
    stop_loss_pct: f64,
    enable_take_profit: bool,
    take_profit_pct: f64,
    enable_trailing_stop: bool,
    trailing_stop_pct: f64,
    force_exit_end: bool,

    continuous: bool,
    oi: bool,

    out_csv: String,
    yearly_returns_csv: String,
}

#[derive(Debug, Clone)]
struct OpenPosition {
    entry_ts: DateTime<FixedOffset>,
    entry_price: f64,
    qty: f64,
    highest_high: f64,
}

#[derive(Debug, Clone)]
struct Trade {
    strategy: String,
    entry_ts: DateTime<FixedOffset>,
    exit_ts: DateTime<FixedOffset>,
    entry_price: f64,
    exit_price: f64,
    qty: f64,
    gross_pnl: f64,
    net_pnl: f64,
    pnl_pct: f64,
    exit_reason: String,
}

#[derive(Debug, Clone)]
struct YearlyReturn {
    year: i32,
    start_equity: f64,
    end_equity: f64,
    return_pct: f64,
}

#[derive(Debug, Deserialize)]
struct HistoricalResponse {
    status: String,
    data: Option<HistoricalData>,
    error_type: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoricalData {
    candles: Vec<Vec<serde_json::Value>>,
}

pub async fn run_backtest_sma(api_key: &str, access_token: &str) -> Result<()> {
    let cfg = load_config()?;

    println!(
        "Running backtest for {}:{} token={} strategy={} interval={} from={} to={}",
        cfg.exchange,
        cfg.symbol,
        cfg.instrument_token,
        cfg.strategy.as_str(),
        cfg.interval,
        cfg.from,
        cfg.to
    );
    println!(
        "Config: fast_sma={} slow_sma={} rmi_length={} rmi_momentum={} start_capital={:.2} commission_pct={:.4} slippage_pct={:.4}",
        cfg.fast_sma,
        cfg.slow_sma,
        cfg.rmi_length,
        cfg.rmi_momentum,
        cfg.start_capital,
        cfg.commission_pct,
        cfg.slippage_pct
    );
    println!(
        "Exits: sma_exit={} rmi_exit={} stop_loss={}({:.2}%) take_profit={}({:.2}%) trailing_stop={}({:.2}%) force_exit_end={}",
        cfg.enable_sma_exit,
        cfg.enable_rmi_exit,
        cfg.enable_stop_loss,
        cfg.stop_loss_pct,
        cfg.enable_take_profit,
        cfg.take_profit_pct,
        cfg.enable_trailing_stop,
        cfg.trailing_stop_pct,
        cfg.force_exit_end
    );

    let candles = fetch_historical_chunked(api_key, access_token, &cfg).await?;
    if candles.len() < 10 {
        bail!("not enough candles fetched");
    }

    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let sma_fast = sma(&closes, cfg.fast_sma);
    let sma_slow = sma(&closes, cfg.slow_sma);
    let rmi_vals = rmi(&closes, cfg.rmi_momentum, cfg.rmi_length);

    let (trades, equity_curve) = run_backtest(&cfg, &candles, &sma_fast, &sma_slow, &rmi_vals)?;
    let yearly = build_yearly_returns(&equity_curve);

    write_trades_csv(&cfg.out_csv, &trades)?;
    write_yearly_returns_csv(&cfg.yearly_returns_csv, &yearly)?;

    for t in &trades {
        println!(
            "[{}] BUY {} @ {:.2} -> SELL {} @ {:.2} qty={:.4} pnl={:.2} ({:.2}%) reason={}",
            t.strategy,
            t.entry_ts,
            t.entry_price,
            t.exit_ts,
            t.exit_price,
            t.qty,
            t.net_pnl,
            t.pnl_pct,
            t.exit_reason
        );
    }

    let final_equity = equity_curve
        .last()
        .map(|x| x.1)
        .unwrap_or(cfg.start_capital);
    let net_pnl = final_equity - cfg.start_capital;
    let ret_pct = if cfg.start_capital > 0.0 {
        (final_equity / cfg.start_capital - 1.0) * 100.0
    } else {
        0.0
    };
    let wins = trades.iter().filter(|t| t.net_pnl > 0.0).count();
    let total = trades.len();
    let win_rate = if total > 0 {
        wins as f64 * 100.0 / total as f64
    } else {
        0.0
    };

    println!();
    println!("========== SUMMARY ==========");
    println!("Strategy: {}", cfg.strategy.as_str());
    println!("Trades: {}", total);
    println!("Win rate: {:.2}%", win_rate);
    println!("Final equity: {:.2}", final_equity);
    println!("Net PnL: {:.2}", net_pnl);
    println!("Return: {:.2}%", ret_pct);
    println!("Trades CSV: {}", cfg.out_csv);
    println!("Yearly CSV: {}", cfg.yearly_returns_csv);

    println!();
    println!("====== YEARLY RETURNS ======");
    for y in &yearly {
        println!(
            "{}  start={:.2} end={:.2} return={:.2}%",
            y.year, y.start_equity, y.end_equity, y.return_pct
        );
    }

    Ok(())
}

fn load_config() -> Result<Config> {
    let now_ist = Local::now().with_timezone(&ist());

    let lookback_years: i64 = env_parse("BACKTEST_LOOKBACK_YEARS", 5_i64)?;
    let from = match env::var("BACKTEST_FROM") {
        Ok(s) if !s.trim().is_empty() => parse_day_start(&s)?,
        _ => now_ist - Duration::days(365 * lookback_years),
    };
    let to = match env::var("BACKTEST_TO") {
        Ok(s) if !s.trim().is_empty() => parse_day_end(&s)?,
        _ => now_ist,
    };

    Ok(Config {
        instrument_token: env_parse("BACKTEST_INSTRUMENT_TOKEN", 738561_u32)?,
        symbol: env::var("BACKTEST_SYMBOL").unwrap_or_else(|_| "RELIANCE".to_string()),
        exchange: env::var("BACKTEST_EXCHANGE").unwrap_or_else(|_| "NSE".to_string()),
        interval: env::var("BACKTEST_INTERVAL").unwrap_or_else(|_| "day".to_string()),

        from,
        to,
        strategy: StrategyKind::from_env(),

        fast_sma: env_parse("BACKTEST_FAST_SMA", 20_usize)?,
        slow_sma: env_parse("BACKTEST_SLOW_SMA", 50_usize)?,

        rmi_length: env_parse("BACKTEST_RMI_LENGTH", 14_usize)?,
        rmi_momentum: env_parse("BACKTEST_RMI_MOMENTUM", 5_usize)?,
        rmi_buy_level: env_parse("BACKTEST_RMI_BUY_LEVEL", 30.0_f64)?,
        rmi_sell_level: env_parse("BACKTEST_RMI_SELL_LEVEL", 70.0_f64)?,
        enable_rmi_exit: env_parse("BACKTEST_ENABLE_RMI_EXIT", true)?,

        start_capital: env_parse("BACKTEST_START_CAPITAL", 100000.0_f64)?,
        commission_pct: env_parse("BACKTEST_COMMISSION_PCT", 0.02_f64)?,
        slippage_pct: env_parse("BACKTEST_SLIPPAGE_PCT", 0.01_f64)?,

        enable_sma_exit: env_parse("BACKTEST_ENABLE_SMA_EXIT", true)?,
        enable_stop_loss: env_parse("BACKTEST_ENABLE_STOP_LOSS", false)?,
        stop_loss_pct: env_parse("BACKTEST_STOP_LOSS_PCT", 5.0_f64)?,
        enable_take_profit: env_parse("BACKTEST_ENABLE_TAKE_PROFIT", false)?,
        take_profit_pct: env_parse("BACKTEST_TAKE_PROFIT_PCT", 12.0_f64)?,
        enable_trailing_stop: env_parse("BACKTEST_ENABLE_TRAILING_STOP", false)?,
        trailing_stop_pct: env_parse("BACKTEST_TRAILING_STOP_PCT", 4.0_f64)?,
        force_exit_end: env_parse("BACKTEST_FORCE_EXIT_AT_END", true)?,

        continuous: env_parse("BACKTEST_CONTINUOUS", false)?,
        oi: env_parse("BACKTEST_OI", false)?,

        out_csv: env::var("BACKTEST_OUT_CSV").unwrap_or_else(|_| "backtest_trades.csv".to_string()),
        yearly_returns_csv: env::var("BACKTEST_YEARLY_RETURNS_CSV")
            .unwrap_or_else(|_| "backtest_yearly_returns.csv".to_string()),
    })
}

fn parse_day_start(s: &str) -> Result<DateTime<FixedOffset>> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date for start: {s}"))?;
    Ok(ist()
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .ok_or_else(|| anyhow!("invalid local start datetime"))?)
}

fn parse_day_end(s: &str) -> Result<DateTime<FixedOffset>> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date for end: {s}"))?;
    Ok(ist()
        .from_local_datetime(&d.and_hms_opt(23, 59, 59).unwrap())
        .single()
        .ok_or_else(|| anyhow!("invalid local end datetime"))?)
}

fn env_parse<T>(key: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => v
            .parse::<T>()
            .map_err(|e| anyhow!("failed to parse {key}={v}: {e}")),
        _ => Ok(default),
    }
}

fn max_days_per_request(interval: &str) -> i64 {
    match interval {
        "minute" | "2minute" => 60,
        "3minute" | "4minute" | "5minute" | "10minute" => 100,
        "15minute" | "30minute" => 200,
        "60minute" | "hour" | "2hour" | "3hour" | "4hour" => 400,
        "day" | "week" => 2000,
        _ => 200,
    }
}

async fn fetch_historical_chunked(
    api_key: &str,
    access_token: &str,
    cfg: &Config,
) -> Result<Vec<Candle>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("X-Kite-Version", HeaderValue::from_static("3"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("token {}:{}", api_key, access_token))?,
    );

    let step_days = max_days_per_request(&cfg.interval) - 1;
    let mut start = cfg.from;
    let mut all = Vec::<Candle>::new();

    while start <= cfg.to {
        let end = std::cmp::min(start + Duration::days(step_days), cfg.to);
        println!("Fetching chunk: {} -> {}", start, end);

        let chunk = fetch_historical_one(
            &client,
            &headers,
            cfg.instrument_token,
            &cfg.interval,
            start,
            end,
            cfg.continuous,
            cfg.oi,
        )
        .await?;

        all.extend(chunk);

        if end >= cfg.to {
            break;
        }
        start = end + Duration::seconds(1);
    }

    all.sort_by_key(|c| c.ts);
    all.dedup_by(|a, b| a.ts == b.ts);

    Ok(all)
}

async fn fetch_historical_one(
    client: &reqwest::Client,
    headers: &HeaderMap,
    instrument_token: u32,
    interval: &str,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
    continuous: bool,
    oi: bool,
) -> Result<Vec<Candle>> {
    let url = format!(
        "https://api.kite.trade/instruments/historical/{}/{}",
        instrument_token, interval
    );

    let resp = client
        .get(url)
        .headers(headers.clone())
        .query(&[
            ("from", from.format("%Y-%m-%d %H:%M:%S").to_string()),
            ("to", to.format("%Y-%m-%d %H:%M:%S").to_string()),
            ("continuous", if continuous { "1" } else { "0" }.to_string()),
            ("oi", if oi { "1" } else { "0" }.to_string()),
        ])
        .send()
        .await
        .context("historical request failed")?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        bail!("historical failed (HTTP {}): {}", status, text);
    }

    let parsed: HistoricalResponse =
        serde_json::from_str(&text).context("failed to parse historical response")?;

    if parsed.status != "success" {
        bail!(
            "historical API error: type={:?} message={:?}",
            parsed.error_type,
            parsed.message
        );
    }

    let data = parsed
        .data
        .ok_or_else(|| anyhow!("historical response missing data"))?;

    let mut candles = Vec::with_capacity(data.candles.len());
    for row in data.candles {
        if row.len() < 6 {
            continue;
        }

        let ts_str = row[0]
            .as_str()
            .ok_or_else(|| anyhow!("candle timestamp missing"))?;

        let ts = DateTime::parse_from_rfc3339(ts_str)
            .or_else(|_| DateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%z"))
            .with_context(|| format!("failed to parse timestamp: {ts_str}"))?;

        candles.push(Candle {
            ts,
            open: as_f64(&row[1])?,
            high: as_f64(&row[2])?,
            low: as_f64(&row[3])?,
            close: as_f64(&row[4])?,
            volume: as_f64(&row[5])?,
            oi: if row.len() > 6 {
                Some(as_f64(&row[6])?)
            } else {
                None
            },
        });
    }

    Ok(candles)
}

fn as_f64(v: &serde_json::Value) -> Result<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|x| x as f64))
        .ok_or_else(|| anyhow!("expected numeric value, got {}", v))
}

fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }

    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values[i];
        if i >= period {
            sum -= values[i - period];
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

fn rma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }

    let mut sum = 0.0;
    for v in values.iter().take(period) {
        sum += *v;
    }

    let mut prev = sum / period as f64;
    out[period - 1] = Some(prev);

    for i in period..values.len() {
        prev = ((period as f64 - 1.0) * prev + values[i]) / period as f64;
        out[i] = Some(prev);
    }

    out
}

fn rmi(closes: &[f64], momentum: usize, length: usize) -> Vec<Option<f64>> {
    let n = closes.len();
    let mut up = vec![0.0; n];
    let mut down = vec![0.0; n];

    if momentum == 0 || n <= momentum {
        return vec![None; n];
    }

    for i in momentum..n {
        let diff = closes[i] - closes[i - momentum];
        if diff > 0.0 {
            up[i] = diff;
        } else {
            down[i] = -diff;
        }
    }

    let avg_up = rma(&up, length);
    let avg_down = rma(&down, length);

    let mut out = vec![None; n];
    for i in 0..n {
        let Some(u) = avg_up[i] else { continue };
        let Some(d) = avg_down[i] else { continue };

        out[i] = if d == 0.0 {
            Some(100.0)
        } else {
            let rm = u / d;
            Some(100.0 * rm / (1.0 + rm))
        };
    }

    out
}

fn run_backtest(
    cfg: &Config,
    candles: &[Candle],
    sma_fast: &[Option<f64>],
    sma_slow: &[Option<f64>],
    rmi_vals: &[Option<f64>],
) -> Result<(Vec<Trade>, Vec<(DateTime<FixedOffset>, f64)>)> {
    let commission = cfg.commission_pct / 100.0;
    let slippage = cfg.slippage_pct / 100.0;

    let mut cash = cfg.start_capital;
    let mut pos: Option<OpenPosition> = None;
    let mut trades = Vec::<Trade>::new();
    let mut equity_curve = Vec::<(DateTime<FixedOffset>, f64)>::new();

    for i in 1..candles.len() {
        let prev = &candles[i - 1];
        let cur = &candles[i];

        let mut exit_reason: Option<&str> = None;
        let mut exit_price: Option<f64> = None;

        if let Some(p) = &pos {
            if cfg.enable_stop_loss {
                let stop = p.entry_price * (1.0 - cfg.stop_loss_pct / 100.0);
                if cur.low <= stop {
                    exit_reason = Some("stop_loss");
                    exit_price = Some(stop * (1.0 - slippage));
                }
            }

            if exit_reason.is_none() && cfg.enable_trailing_stop {
                let trail = p.highest_high * (1.0 - cfg.trailing_stop_pct / 100.0);
                if cur.low <= trail {
                    exit_reason = Some("trailing_stop");
                    exit_price = Some(trail * (1.0 - slippage));
                }
            }

            if exit_reason.is_none() && cfg.enable_take_profit {
                let tp = p.entry_price * (1.0 + cfg.take_profit_pct / 100.0);
                if cur.high >= tp {
                    exit_reason = Some("take_profit");
                    exit_price = Some(tp * (1.0 - slippage));
                }
            }

            if exit_reason.is_none() {
                match cfg.strategy {
                    StrategyKind::Sma => {
                        if cfg.enable_sma_exit {
                            if let (Some(pf), Some(ps), Some(cf), Some(cs)) =
                                (sma_fast[i - 1], sma_slow[i - 1], sma_fast[i], sma_slow[i])
                            {
                                if pf >= ps && cf < cs {
                                    exit_reason = Some("sma_cross_down");
                                    exit_price = Some(cur.close * (1.0 - slippage));
                                }
                            }
                        }
                    }
                    StrategyKind::Rmi => {
                        if cfg.enable_rmi_exit {
                            if let (Some(pr), Some(cr)) = (rmi_vals[i - 1], rmi_vals[i]) {
                                if pr > cfg.rmi_sell_level && cr <= cfg.rmi_sell_level {
                                    exit_reason = Some("rmi_cross_down");
                                    exit_price = Some(cur.close * (1.0 - slippage));
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(reason) = exit_reason {
            if let Some(p) = pos.take() {
                let px = exit_price.unwrap();
                let gross_value = p.qty * px;
                let exit_fee = gross_value * commission;
                cash += gross_value - exit_fee;

                let gross_pnl = (px - p.entry_price) * p.qty;
                let net_pnl =
                    cash - cfg.start_capital - trades.iter().map(|t| t.net_pnl).sum::<f64>();

                let pnl_pct = if p.entry_price > 0.0 {
                    (px / p.entry_price - 1.0) * 100.0
                } else {
                    0.0
                };

                trades.push(Trade {
                    strategy: cfg.strategy.as_str().to_string(),
                    entry_ts: p.entry_ts,
                    exit_ts: cur.ts,
                    entry_price: p.entry_price,
                    exit_price: px,
                    qty: p.qty,
                    gross_pnl,
                    net_pnl,
                    pnl_pct,
                    exit_reason: reason.to_string(),
                });
            }
        }

        if let Some(p) = &mut pos {
            if cur.high > p.highest_high {
                p.highest_high = cur.high;
            }
        }

        if pos.is_none() {
            let buy_signal = match cfg.strategy {
                StrategyKind::Sma => {
                    if let (Some(pf), Some(ps), Some(cf), Some(cs)) =
                        (sma_fast[i - 1], sma_slow[i - 1], sma_fast[i], sma_slow[i])
                    {
                        pf <= ps && cf > cs
                    } else {
                        false
                    }
                }
                StrategyKind::Rmi => {
                    if let (Some(pr), Some(cr)) = (rmi_vals[i - 1], rmi_vals[i]) {
                        pr < cfg.rmi_buy_level && cr >= cfg.rmi_buy_level
                    } else {
                        false
                    }
                }
            };

            if buy_signal && cash > 0.0 {
                let entry_px = cur.close * (1.0 + slippage);
                let qty = cash / (entry_px * (1.0 + commission));
                let gross_cost = qty * entry_px;
                let entry_fee = gross_cost * commission;
                cash -= gross_cost + entry_fee;

                pos = Some(OpenPosition {
                    entry_ts: cur.ts,
                    entry_price: entry_px,
                    qty,
                    highest_high: cur.high,
                });
            }
        }

        let equity = match &pos {
            Some(p) => cash + p.qty * cur.close,
            None => cash,
        };
        equity_curve.push((cur.ts, equity));
    }

    if cfg.force_exit_end {
        if let (Some(last), Some(p)) = (candles.last(), pos.take()) {
            let px = last.close * (1.0 - slippage);
            let gross_value = p.qty * px;
            let exit_fee = gross_value * commission;
            cash += gross_value - exit_fee;

            let gross_pnl = (px - p.entry_price) * p.qty;
            let net_pnl = cash - cfg.start_capital - trades.iter().map(|t| t.net_pnl).sum::<f64>();
            let pnl_pct = if p.entry_price > 0.0 {
                (px / p.entry_price - 1.0) * 100.0
            } else {
                0.0
            };

            trades.push(Trade {
                strategy: cfg.strategy.as_str().to_string(),
                entry_ts: p.entry_ts,
                exit_ts: last.ts,
                entry_price: p.entry_price,
                exit_price: px,
                qty: p.qty,
                gross_pnl,
                net_pnl,
                pnl_pct,
                exit_reason: "force_exit_end".to_string(),
            });

            equity_curve.push((last.ts, cash));
        }
    }

    Ok((trades, equity_curve))
}

fn build_yearly_returns(curve: &[(DateTime<FixedOffset>, f64)]) -> Vec<YearlyReturn> {
    let mut years: BTreeMap<i32, (f64, f64)> = BTreeMap::new();

    for (ts, equity) in curve {
        years
            .entry(ts.year())
            .and_modify(|(_, end)| *end = *equity)
            .or_insert((*equity, *equity));
    }

    years
        .into_iter()
        .map(|(year, (start_equity, end_equity))| YearlyReturn {
            year,
            start_equity,
            end_equity,
            return_pct: if start_equity > 0.0 {
                (end_equity / start_equity - 1.0) * 100.0
            } else {
                0.0
            },
        })
        .collect()
}

fn write_trades_csv(path: &str, trades: &[Trade]) -> Result<()> {
    let mut wtr = Writer::from_path(path)?;
    wtr.write_record([
        "strategy",
        "entry_ts",
        "exit_ts",
        "entry_price",
        "exit_price",
        "qty",
        "gross_pnl",
        "net_pnl",
        "pnl_pct",
        "exit_reason",
    ])?;

    for t in trades {
        wtr.write_record([
            t.strategy.as_str(),
            &t.entry_ts.to_rfc3339(),
            &t.exit_ts.to_rfc3339(),
            &format!("{:.6}", t.entry_price),
            &format!("{:.6}", t.exit_price),
            &format!("{:.6}", t.qty),
            &format!("{:.6}", t.gross_pnl),
            &format!("{:.6}", t.net_pnl),
            &format!("{:.6}", t.pnl_pct),
            t.exit_reason.as_str(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn write_yearly_returns_csv(path: &str, rows: &[YearlyReturn]) -> Result<()> {
    let mut wtr = Writer::from_path(path)?;
    wtr.write_record(["year", "start_equity", "end_equity", "return_pct"])?;

    for y in rows {
        wtr.write_record([
            y.year.to_string(),
            format!("{:.6}", y.start_equity),
            format!("{:.6}", y.end_equity),
            format!("{:.6}", y.return_pct),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}
