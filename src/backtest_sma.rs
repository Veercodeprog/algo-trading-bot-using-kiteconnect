use anyhow::{Context, Result, bail};
use chrono::{FixedOffset, Local, NaiveDate, TimeZone};
use std::fs::File;
use std::io::Write;

use crate::{history, instruments};

#[derive(Debug, Clone)]
struct Trade {
    entry_bar: usize,
    exit_bar: usize,
    qty: u64,
    entry_price: f64,
    exit_price: f64,
    gross_pnl: f64,
    net_pnl: f64,
    pnl_pct: f64,
}

#[derive(Debug, Clone)]
struct BacktestSummary {
    symbol: String,
    exchange: String,
    interval: String,
    start_capital: f64,
    final_equity: f64,
    total_return_pct: f64,
    total_trades: usize,
    winning_trades: usize,
    losing_trades: usize,
    win_rate_pct: f64,
    max_drawdown_pct: f64,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr + Copy,
{
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y"
        ),
        Err(_) => default,
    }
}

fn sma_at(closes: &[f64], period: usize, idx: usize) -> Option<f64> {
    if period == 0 || idx + 1 < period {
        return None;
    }
    let start = idx + 1 - period;
    let sum: f64 = closes[start..=idx].iter().sum();
    Some(sum / period as f64)
}

fn write_trades_csv(path: &str, trades: &[Trade]) -> Result<()> {
    let mut f = File::create(path).with_context(|| format!("failed to create {path}"))?;
    writeln!(
        f,
        "entry_bar,exit_bar,qty,entry_price,exit_price,gross_pnl,net_pnl,pnl_pct"
    )?;

    for t in trades {
        writeln!(
            f,
            "{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4}",
            t.entry_bar,
            t.exit_bar,
            t.qty,
            t.entry_price,
            t.exit_price,
            t.gross_pnl,
            t.net_pnl,
            t.pnl_pct
        )?;
    }

    Ok(())
}

fn print_summary(summary: &BacktestSummary) {
    println!("--- SMA Backtest Summary ---");
    println!(
        "Instrument        : {}:{}",
        summary.exchange, summary.symbol
    );
    println!("Interval          : {}", summary.interval);
    println!("Start Capital     : {:.2}", summary.start_capital);
    println!("Final Equity      : {:.2}", summary.final_equity);
    println!("Total Return %    : {:.2}", summary.total_return_pct);
    println!("Total Trades      : {}", summary.total_trades);
    println!("Winning Trades    : {}", summary.winning_trades);
    println!("Losing Trades     : {}", summary.losing_trades);
    println!("Win Rate %        : {:.2}", summary.win_rate_pct);
    println!("Max Drawdown %    : {:.2}", summary.max_drawdown_pct);
}

pub async fn run_backtest_sma(
    api_key: &str,
    access_token: &str,
    instruments_cache: &[instruments::Instrument],
) -> Result<()> {
    let exchange = env_or("BACKTEST_EXCHANGE", "NSE");
    let symbol = env_or("BACKTEST_SYMBOL", "RELIANCE");
    let interval = env_or("BACKTEST_INTERVAL", "day");

    let from_str = env_or("BACKTEST_FROM", "2023-01-01");
    let to_str = env_or("BACKTEST_TO", &Local::now().format("%Y-%m-%d").to_string());

    let fast_period: usize = env_parse("BACKTEST_MA_FAST", 20usize);
    let slow_period: usize = env_parse("BACKTEST_MA_SLOW", 50usize);

    let start_capital: f64 = env_parse("BACKTEST_START_CAPITAL", 100_000.0_f64);
    let commission_pct: f64 = env_parse("BACKTEST_COMMISSION_PCT", 0.0_f64);
    let slippage_pct: f64 = env_parse("BACKTEST_SLIPPAGE_PCT", 0.0_f64);

    let continuous = env_bool("BACKTEST_CONTINUOUS", false);
    let oi = env_bool("BACKTEST_OI", false);

    let out_csv = env_or("BACKTEST_OUT_CSV", "backtest_sma_trades.csv");

    if fast_period == 0 || slow_period == 0 {
        bail!("BACKTEST_MA_FAST and BACKTEST_MA_SLOW must be > 0");
    }
    if fast_period >= slow_period {
        bail!("BACKTEST_MA_FAST must be smaller than BACKTEST_MA_SLOW");
    }
    if start_capital <= 0.0 {
        bail!("BACKTEST_START_CAPITAL must be > 0");
    }

    let token = instruments::find_instrument_token(instruments_cache, &exchange, &symbol)
        .with_context(|| format!("instrument_token not found for {exchange}:{symbol}"))?;

    let from_date = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
        .with_context(|| format!("invalid BACKTEST_FROM: {from_str}"))?;
    let to_date = NaiveDate::parse_from_str(&to_str, "%Y-%m-%d")
        .with_context(|| format!("invalid BACKTEST_TO: {to_str}"))?;

    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
    let from_dt = ist
        .from_local_datetime(&from_date.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .context("invalid BACKTEST_FROM datetime")?;
    let to_dt = ist
        .from_local_datetime(&to_date.and_hms_opt(23, 59, 59).unwrap())
        .single()
        .context("invalid BACKTEST_TO datetime")?;

    println!(
        "Running backtest for {}:{} token={} interval={} from={} to={} fast={} slow={}",
        exchange, symbol, token, interval, from_dt, to_dt, fast_period, slow_period
    );

    let candles = history::fetch_historical(
        api_key,
        access_token,
        token,
        &interval,
        from_dt,
        to_dt,
        continuous,
        oi,
    )
    .await?;

    if candles.len() < slow_period + 2 {
        bail!(
            "not enough candles: got {}, need at least {}",
            candles.len(),
            slow_period + 2
        );
    }

    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

    let mut cash = start_capital;
    let mut qty: u64 = 0;
    let mut entry_price = 0.0;
    let mut entry_bar = 0usize;
    let mut trades: Vec<Trade> = Vec::new();

    let mut peak_equity = start_capital;
    let mut max_drawdown_pct = 0.0_f64;

    for i in 1..closes.len() {
        let fast_prev = match sma_at(&closes, fast_period, i - 1) {
            Some(v) => v,
            None => continue,
        };
        let slow_prev = match sma_at(&closes, slow_period, i - 1) {
            Some(v) => v,
            None => continue,
        };
        let fast_now = match sma_at(&closes, fast_period, i) {
            Some(v) => v,
            None => continue,
        };
        let slow_now = match sma_at(&closes, slow_period, i) {
            Some(v) => v,
            None => continue,
        };

        let px = closes[i];

        let cross_up = fast_prev <= slow_prev && fast_now > slow_now;
        let cross_down = fast_prev >= slow_prev && fast_now < slow_now;

        if qty == 0 && cross_up {
            let buy_px = px * (1.0 + slippage_pct / 100.0);
            let per_share_cost = buy_px * (1.0 + commission_pct / 100.0);
            let buy_qty = (cash / per_share_cost).floor() as u64;

            if buy_qty > 0 {
                let total_cost = buy_qty as f64 * per_share_cost;
                cash -= total_cost;
                qty = buy_qty;
                entry_price = buy_px;
                entry_bar = i;

                println!(
                    "BUY  | bar={} close={:.2} exec={:.2} qty={} cash_left={:.2}",
                    i, px, buy_px, qty, cash
                );
            }
        } else if qty > 0 && cross_down {
            let sell_px = px * (1.0 - slippage_pct / 100.0);
            let gross_value = qty as f64 * sell_px;
            let sell_fee = gross_value * (commission_pct / 100.0);
            let net_value = gross_value - sell_fee;

            cash += net_value;

            let gross_pnl = (sell_px - entry_price) * qty as f64;
            let buy_fee = (qty as f64 * entry_price) * (commission_pct / 100.0);
            let net_pnl = gross_pnl - buy_fee - sell_fee;
            let pnl_pct = if entry_price > 0.0 {
                ((sell_px - entry_price) / entry_price) * 100.0
            } else {
                0.0
            };

            trades.push(Trade {
                entry_bar,
                exit_bar: i,
                qty,
                entry_price,
                exit_price: sell_px,
                gross_pnl,
                net_pnl,
                pnl_pct,
            });

            println!(
                "SELL | bar={} close={:.2} exec={:.2} qty={} net_pnl={:.2} cash={:.2}",
                i, px, sell_px, qty, net_pnl, cash
            );

            qty = 0;
            entry_price = 0.0;
        }

        let equity = if qty > 0 {
            cash + qty as f64 * px
        } else {
            cash
        };

        if equity > peak_equity {
            peak_equity = equity;
        }
        let dd = if peak_equity > 0.0 {
            ((peak_equity - equity) / peak_equity) * 100.0
        } else {
            0.0
        };
        if dd > max_drawdown_pct {
            max_drawdown_pct = dd;
        }
    }

    if qty > 0 {
        let i = closes.len() - 1;
        let px = closes[i];
        let sell_px = px * (1.0 - slippage_pct / 100.0);
        let gross_value = qty as f64 * sell_px;
        let sell_fee = gross_value * (commission_pct / 100.0);
        let net_value = gross_value - sell_fee;

        cash += net_value;

        let gross_pnl = (sell_px - entry_price) * qty as f64;
        let buy_fee = (qty as f64 * entry_price) * (commission_pct / 100.0);
        let net_pnl = gross_pnl - buy_fee - sell_fee;
        let pnl_pct = if entry_price > 0.0 {
            ((sell_px - entry_price) / entry_price) * 100.0
        } else {
            0.0
        };

        trades.push(Trade {
            entry_bar,
            exit_bar: i,
            qty,
            entry_price,
            exit_price: sell_px,
            gross_pnl,
            net_pnl,
            pnl_pct,
        });

        println!(
            "FORCE EXIT | bar={} close={:.2} exec={:.2} qty={} net_pnl={:.2} cash={:.2}",
            i, px, sell_px, qty, net_pnl, cash
        );
    }

    let final_equity = cash;
    let total_return_pct = ((final_equity - start_capital) / start_capital) * 100.0;
    let winning_trades = trades.iter().filter(|t| t.net_pnl > 0.0).count();
    let losing_trades = trades.iter().filter(|t| t.net_pnl <= 0.0).count();
    let total_trades = trades.len();
    let win_rate_pct = if total_trades > 0 {
        (winning_trades as f64 / total_trades as f64) * 100.0
    } else {
        0.0
    };

    let summary = BacktestSummary {
        symbol,
        exchange,
        interval,
        start_capital,
        final_equity,
        total_return_pct,
        total_trades,
        winning_trades,
        losing_trades,
        win_rate_pct,
        max_drawdown_pct,
    };

    write_trades_csv(&out_csv, &trades)?;
    print_summary(&summary);
    println!("Wrote trades CSV to {}", out_csv);

    Ok(())
}
