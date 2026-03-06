use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;

use crate::{history, instruments};

#[derive(Debug, Clone)]
struct Trade {
    entry_bar: usize,
    exit_bar: usize,
    entry_ts: DateTime<FixedOffset>,
    exit_ts: DateTime<FixedOffset>,
    qty: u64,
    entry_price: f64,
    exit_price: f64,
    gross_pnl: f64,
    net_pnl: f64,
    pnl_pct: f64,
}

#[derive(Debug, Clone)]
struct EquityPoint {
    ts: DateTime<FixedOffset>,
    equity: f64,
}

#[derive(Debug, Clone)]
struct YearlyReturn {
    year: i32,
    from_ts: DateTime<FixedOffset>,
    to_ts: DateTime<FixedOffset>,
    start_equity: f64,
    end_equity: f64,
    return_pct: f64,
}

#[derive(Debug, Clone)]
struct BacktestSummary {
    symbol: String,
    exchange: String,
    interval: String,
    fast_sma: usize,
    slow_sma: usize,
    start_capital: f64,
    final_equity: f64,
    total_return_pct: f64,
    total_trades: usize,
    winning_trades: usize,
    losing_trades: usize,
    win_rate_pct: f64,
    max_drawdown_pct: f64,
    yearly_returns: Vec<YearlyReturn>,
}

#[derive(Debug, Clone)]
struct LocalBar {
    ts: DateTime<FixedOffset>,
    close: f64,
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

fn fmt_ts(ts: &DateTime<FixedOffset>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S %:z").to_string()
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
        "entry_bar,exit_bar,entry_date,exit_date,qty,entry_price,exit_price,gross_pnl,net_pnl,pnl_pct"
    )?;

    for t in trades {
        writeln!(
            f,
            "{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4}",
            t.entry_bar,
            t.exit_bar,
            fmt_ts(&t.entry_ts),
            fmt_ts(&t.exit_ts),
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

fn compute_yearly_returns(equity_curve: &[EquityPoint]) -> Vec<YearlyReturn> {
    let mut yearly_map: BTreeMap<i32, Vec<&EquityPoint>> = BTreeMap::new();

    for p in equity_curve {
        yearly_map.entry(p.ts.year()).or_default().push(p);
    }

    let mut yearly_returns = Vec::new();

    for (year, points) in yearly_map {
        if points.is_empty() {
            continue;
        }

        let first = points.first().unwrap();
        let last = points.last().unwrap();

        let return_pct = if first.equity != 0.0 {
            ((last.equity - first.equity) / first.equity) * 100.0
        } else {
            0.0
        };

        yearly_returns.push(YearlyReturn {
            year,
            from_ts: first.ts,
            to_ts: last.ts,
            start_equity: first.equity,
            end_equity: last.equity,
            return_pct,
        });
    }

    yearly_returns
}

fn print_summary(summary: &BacktestSummary) {
    println!("\n================ SMA BACKTEST SUMMARY ================");
    println!(
        "Instrument        : {}:{}",
        summary.exchange, summary.symbol
    );
    println!("Interval          : {}", summary.interval);
    println!(
        "Strategy          : SMA {} / SMA {}",
        summary.fast_sma, summary.slow_sma
    );
    println!("Start Capital     : {:.2}", summary.start_capital);
    println!("Final Equity      : {:.2}", summary.final_equity);
    println!("Total Return %    : {:.2}", summary.total_return_pct);
    println!("Total Trades      : {}", summary.total_trades);
    println!("Winning Trades    : {}", summary.winning_trades);
    println!("Losing Trades     : {}", summary.losing_trades);
    println!("Win Rate %        : {:.2}", summary.win_rate_pct);
    println!("Max Drawdown %    : {:.2}", summary.max_drawdown_pct);

    println!("\n---------------- YEAR-WISE RETURNS ----------------");
    println!(
        "{:<8} {:<25} {:<25} {:>14} {:>14} {:>12}",
        "Year", "From Date", "To Date", "Start Eq", "End Eq", "Return %"
    );

    for y in &summary.yearly_returns {
        println!(
            "{:<8} {:<25} {:<25} {:>14.2} {:>14.2} {:>12.2}",
            y.year,
            fmt_ts(&y.from_ts),
            fmt_ts(&y.to_ts),
            y.start_equity,
            y.end_equity,
            y.return_pct
        );
    }
}

fn resolve_date_range() -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();

    let explicit_from = std::env::var("BACKTEST_FROM").ok();
    let explicit_to = std::env::var("BACKTEST_TO").ok();

    if explicit_from.is_some() || explicit_to.is_some() {
        let from_str = explicit_from.unwrap_or_else(|| "2006-01-01".to_string());
        let to_str = explicit_to.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());

        let from_date = NaiveDate::parse_from_str(&from_str, "%Y-%m-%d")
            .with_context(|| format!("invalid BACKTEST_FROM: {from_str}"))?;
        let to_date = NaiveDate::parse_from_str(&to_str, "%Y-%m-%d")
            .with_context(|| format!("invalid BACKTEST_TO: {to_str}"))?;

        let from_dt = ist
            .from_local_datetime(&from_date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .context("invalid BACKTEST_FROM datetime")?;

        let to_dt = ist
            .from_local_datetime(&to_date.and_hms_opt(23, 59, 59).unwrap())
            .single()
            .context("invalid BACKTEST_TO datetime")?;

        return Ok((from_dt, to_dt));
    }

    let lookback_years: i64 = env_parse("BACKTEST_LOOKBACK_YEARS", 20_i64);
    if lookback_years <= 0 {
        bail!("BACKTEST_LOOKBACK_YEARS must be > 0");
    }

    let to_dt = Local::now().with_timezone(&ist);
    let from_dt = to_dt - Duration::days(lookback_years * 365);

    Ok((from_dt, to_dt))
}

pub async fn run_backtest_sma(
    api_key: &str,
    access_token: &str,
    instruments_cache: &[instruments::Instrument],
) -> Result<()> {
    let exchange = env_or("BACKTEST_EXCHANGE", "NSE");
    let symbol = env_or("BACKTEST_SYMBOL", "RELIANCE");
    let interval = env_or("BACKTEST_INTERVAL", "day");

    let fast_period: usize = env_parse("BACKTEST_FAST_SMA", 20usize);
    let slow_period: usize = env_parse("BACKTEST_SLOW_SMA", 50usize);

    let start_capital: f64 = env_parse("BACKTEST_START_CAPITAL", 100_000.0_f64);
    let commission_pct: f64 = env_parse("BACKTEST_COMMISSION_PCT", 0.0_f64);
    let slippage_pct: f64 = env_parse("BACKTEST_SLIPPAGE_PCT", 0.0_f64);

    let continuous = env_bool("BACKTEST_CONTINUOUS", false);
    let oi = env_bool("BACKTEST_OI", false);

    let out_csv = env_or("BACKTEST_OUT_CSV", "backtest_sma_trades.csv");

    if fast_period == 0 || slow_period == 0 {
        bail!("BACKTEST_FAST_SMA and BACKTEST_SLOW_SMA must be > 0");
    }
    if fast_period >= slow_period {
        bail!("BACKTEST_FAST_SMA must be smaller than BACKTEST_SLOW_SMA");
    }
    if start_capital <= 0.0 {
        bail!("BACKTEST_START_CAPITAL must be > 0");
    }

    let token = instruments::find_instrument_token(instruments_cache, &exchange, &symbol)
        .with_context(|| format!("instrument_token not found for {exchange}:{symbol}"))?;

    let (from_dt, to_dt) = resolve_date_range()?;

    println!(
        "Running backtest for {}:{} token={} interval={} from={} to={} fast_sma={} slow_sma={}",
        exchange,
        symbol,
        token,
        interval,
        fmt_ts(&from_dt),
        fmt_ts(&to_dt),
        fast_period,
        slow_period
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

    let bars: Vec<LocalBar> = candles
        .iter()
        .map(|c| LocalBar {
            ts: c.time,
            close: c.close,
        })
        .collect();

    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();

    let mut cash = start_capital;
    let mut qty: u64 = 0;
    let mut entry_price = 0.0;
    let mut entry_bar = 0usize;
    let mut entry_ts = bars[0].ts;
    let mut trades: Vec<Trade> = Vec::new();

    let mut peak_equity = start_capital;
    let mut max_drawdown_pct = 0.0_f64;

    let mut equity_curve: Vec<EquityPoint> = vec![EquityPoint {
        ts: bars[0].ts,
        equity: start_capital,
    }];

    for i in 1..bars.len() {
        let fast_prev = match sma_at(&closes, fast_period, i - 1) {
            Some(v) => v,
            None => {
                equity_curve.push(EquityPoint {
                    ts: bars[i].ts,
                    equity: if qty > 0 {
                        cash + qty as f64 * bars[i].close
                    } else {
                        cash
                    },
                });
                continue;
            }
        };

        let slow_prev = match sma_at(&closes, slow_period, i - 1) {
            Some(v) => v,
            None => {
                equity_curve.push(EquityPoint {
                    ts: bars[i].ts,
                    equity: if qty > 0 {
                        cash + qty as f64 * bars[i].close
                    } else {
                        cash
                    },
                });
                continue;
            }
        };

        let fast_now = match sma_at(&closes, fast_period, i) {
            Some(v) => v,
            None => {
                equity_curve.push(EquityPoint {
                    ts: bars[i].ts,
                    equity: if qty > 0 {
                        cash + qty as f64 * bars[i].close
                    } else {
                        cash
                    },
                });
                continue;
            }
        };

        let slow_now = match sma_at(&closes, slow_period, i) {
            Some(v) => v,
            None => {
                equity_curve.push(EquityPoint {
                    ts: bars[i].ts,
                    equity: if qty > 0 {
                        cash + qty as f64 * bars[i].close
                    } else {
                        cash
                    },
                });
                continue;
            }
        };

        let close_price = bars[i].close;
        let ts = bars[i].ts;

        let buy_signal = fast_prev <= slow_prev && fast_now > slow_now;
        let sell_signal = fast_prev >= slow_prev && fast_now < slow_now;

        if qty == 0 && buy_signal {
            let buy_px = close_price * (1.0 + slippage_pct / 100.0);
            let per_share_cost = buy_px * (1.0 + commission_pct / 100.0);
            let buy_qty = (cash / per_share_cost).floor() as u64;

            if buy_qty > 0 {
                let total_cost = buy_qty as f64 * per_share_cost;
                cash -= total_cost;
                qty = buy_qty;
                entry_price = buy_px;
                entry_bar = i;
                entry_ts = ts;

                println!(
                    "BUY  | date={} | bar={} | close={:.2} | exec={:.2} | qty={} | fast_sma={:.2} | slow_sma={:.2}",
                    fmt_ts(&ts),
                    i,
                    close_price,
                    buy_px,
                    qty,
                    fast_now,
                    slow_now
                );
            }
        } else if qty > 0 && sell_signal {
            let sell_px = close_price * (1.0 - slippage_pct / 100.0);
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
                entry_ts,
                exit_ts: ts,
                qty,
                entry_price,
                exit_price: sell_px,
                gross_pnl,
                net_pnl,
                pnl_pct,
            });

            println!(
                "SELL | date={} | bar={} | close={:.2} | exec={:.2} | qty={} | net_pnl={:.2} | pnl_pct={:.2} | fast_sma={:.2} | slow_sma={:.2}",
                fmt_ts(&ts),
                i,
                close_price,
                sell_px,
                qty,
                net_pnl,
                pnl_pct,
                fast_now,
                slow_now
            );

            qty = 0;
            entry_price = 0.0;
        }

        let equity = if qty > 0 {
            cash + qty as f64 * close_price
        } else {
            cash
        };

        if equity > peak_equity {
            peak_equity = equity;
        }

        let drawdown_pct = if peak_equity > 0.0 {
            ((peak_equity - equity) / peak_equity) * 100.0
        } else {
            0.0
        };

        if drawdown_pct > max_drawdown_pct {
            max_drawdown_pct = drawdown_pct;
        }

        equity_curve.push(EquityPoint { ts, equity });
    }

    if qty > 0 {
        let i = bars.len() - 1;
        let close_price = bars[i].close;
        let ts = bars[i].ts;

        let sell_px = close_price * (1.0 - slippage_pct / 100.0);
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
            entry_ts,
            exit_ts: ts,
            qty,
            entry_price,
            exit_price: sell_px,
            gross_pnl,
            net_pnl,
            pnl_pct,
        });

        println!(
            "FORCE EXIT | date={} | bar={} | close={:.2} | exec={:.2} | qty={} | net_pnl={:.2} | pnl_pct={:.2}",
            fmt_ts(&ts),
            i,
            close_price,
            sell_px,
            qty,
            net_pnl,
            pnl_pct
        );

        equity_curve.push(EquityPoint { ts, equity: cash });
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

    let yearly_returns = compute_yearly_returns(&equity_curve);

    let summary = BacktestSummary {
        symbol,
        exchange,
        interval,
        fast_sma: fast_period,
        slow_sma: slow_period,
        start_capital,
        final_equity,
        total_return_pct,
        total_trades,
        winning_trades,
        losing_trades,
        win_rate_pct,
        max_drawdown_pct,
        yearly_returns,
    };

    write_trades_csv(&out_csv, &trades)?;
    print_summary(&summary);
    println!("\nWrote trades CSV to {}", out_csv);

    Ok(())
}
