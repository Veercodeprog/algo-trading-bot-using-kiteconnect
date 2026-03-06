// src/main.rs
mod account;
mod auth;
mod backtest_sma;
mod bot;
mod broker;
mod config;
mod gtt;
mod history;
mod holdings;
mod instruments;
mod live_sma_trader;
mod margins;
mod market_display;
mod nfo;
mod order_flow;
mod orders;
mod polling_strategy;
mod portfolio;
mod ticker_stream;

mod pretty;
mod ratelimit;
mod session;
mod token_store;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;

use crate::{broker::Broker, config::Config};

#[derive(Parser)]
#[command(name = "zerodha-bot-rs")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Auth,
    Run,
    Margins,
    Logout,
    /// Places a real MARKET BUY then MARKET SELL using env defaults (DANGEROUS; enable explicitly).
    OrderDemo,
    /// Demo: place LIMIT BUY (below LTP), list orders, modify, cancel, show history + trades (REAL APIs).
    OrderLimitManageDemo,
    /// Print net/day positions (like kite.positions()).
    Positions,

    /// Convert position product type (e.g., MIS -> CNC). Uses .env defaults unless overridden.
    ConvertMisToCnc,
    Holdings,
    HoldingsSummary,
    MfHoldings,
    Ticker,
    LiveSmaTrader,
    LowfreqSma,
    CoDemo,
    AmoDemo,
    IcebergDemo,
    GttSingleDemo,
    GttOcoDemo,
    BacktestSma,

    /// Anti-pattern demo: spam LTP to trigger rate-limit errors (for learning only)
    SpamLtp {
        #[arg(long, default_value_t = 200)]
        count: u32,

        #[arg(long, default_value = "NSE:INFY")]
        instrument: String,
    },

    /// Recommended: throttle requests and batch instruments per call
    ThrottleLtp {
        #[arg(long, default_value_t = 5)]
        count: u32,

        /// Quote endpoints are typically ~1 req/sec; keep this >= 1000ms. [web:144]
        #[arg(long, default_value_t = 1100)]
        sleep_ms: u64,

        /// Comma-separated list: NSE:INFY,NSE:RELIANCE
        #[arg(long, value_delimiter = ',', default_value = "NSE:INFY,NSE:RELIANCE")]
        instruments: Vec<String>,
    },
    /// Fetch historical candles and write CSV
    History {
        #[arg(long, default_value = "NSE")]
        exchange: String,

        #[arg(long, default_value = "INFY")]
        symbol: String,

        /// One of: day, minute, 3minute, 5minute, 15minute, 30minute, 60minute [web:167]
        #[arg(long, default_value = "day")]
        interval: String,

        #[arg(long, default_value_t = 90)]
        days: i64,

        #[arg(long, default_value = "historical.csv")]
        out: String,
    },

    /// Fetch 5m/15m/60m and generate a PNG plot (last trading day)
    HistoryMtfPlot {
        #[arg(long, default_value = "NSE")]
        exchange: String,

        #[arg(long, default_value = "INFY")]
        symbol: String,

        #[arg(long, default_value_t = 5)]
        days: i64,

        #[arg(long, default_value = "mtf.png")]
        out: String,
    },
    /// Find front-month NIFTY FUT (NFO), then fetch continuous daily candles with OI and plot Close vs OI.
    NiftyFutOi {
        #[arg(long, default_value = "2024-05-01")]
        from: String,

        #[arg(long, default_value = "2024-05-31")]
        to: String,

        #[arg(long, default_value = "nifty_fut_cont_oi.csv")]
        out_csv: String,

        #[arg(long, default_value = "nifty_fut_cont_oi.png")]
        out_png: String,
    },
    /// Place a stoploss-market exit SELL (SL-M) for an existing long.
    PlaceSlm {
        #[arg(long, default_value = "NSE")]
        exchange: String,
        #[arg(long, default_value = "INFY")]
        symbol: String,
        #[arg(long, default_value_t = 10)]
        qty: u32,
        #[arg(long)]
        trigger: f64,
        #[arg(long, default_value = "MIS")]
        product: String,
    },

    /// Place a stoploss-limit exit SELL (SL) for an existing long.
    PlaceSl {
        #[arg(long, default_value = "NSE")]
        exchange: String,
        #[arg(long, default_value = "INFY")]
        symbol: String,
        #[arg(long, default_value_t = 10)]
        qty: u32,
        #[arg(long)]
        trigger: f64,
        #[arg(long)]
        price: f64,
        #[arg(long, default_value = "MIS")]
        product: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    dotenv().ok();
    let cfg = Config::from_env()?;
    let cli = Cli::parse();
    let cmd = cli.cmd.unwrap_or(Command::Run);
    match cmd {
        Command::Auth => {
            let tok = auth::login_and_store_token(&cfg).await?;
            println!(
                "Saved access_token (len={} chars) to {}",
                tok.access_token.len(),
                cfg.token_path
            );
            let mut broker = Broker::new(&cfg.api_key, &tok.access_token);
            let profile = broker.profile().context("profile failed after auth")?;
            pretty::print_profile(&profile);
        }
        Command::Run => {
            let tok = auth::ensure_token(&cfg).await?;
            let instruments = instruments::load_or_download(
                &cfg.api_key,
                &tok.access_token,
                "instruments_cache.bin",
            )
            .await?;
            instruments::demo_filters(&instruments);
            let mut broker = Broker::new(&cfg.api_key, &tok.access_token);
            let instrument_list = ["NSE:INFY", "NSE:RELIANCE", "BSE:TCS", "NSE:NIFTY 50"];

            let ltp = broker.ltp(&instrument_list)?;
            market_display::print_ltp_table(&ltp);

            let ohlc = broker.ohlc(&instrument_list)?;
            market_display::print_ohlc_table(&ohlc);

            let quote = broker.quote(&instrument_list)?;
            market_display::print_quote_pretty(&quote);

            // Optional: validate + print once at startup like the notebook.
            let profile = broker.profile().context("profile failed")?;
            pretty::print_profile(&profile);

            bot::run_bot(broker)?;
        }
        Command::Margins => {
            let tok = auth::ensure_token(&cfg).await?;
            let mut broker = Broker::new(&cfg.api_key, &tok.access_token);

            println!("\nFetching funds and margins data...");
            let margins = broker.margins(None)?; // None => all segments [web:61]

            let flat = account::flatten_equity_margins(&margins)?;
            account::print_flat_row(&flat);
        }

        Command::Logout => {
            let tok = auth::ensure_token(&cfg).await?;
            let resp = session::logout(&cfg.api_key, &tok.access_token).await?;
            println!("Logout successful: {resp}");
            // Optional: delete token.json so next run forces auth again
            let _ = std::fs::remove_file(&cfg.token_path);
        }
        Command::SpamLtp { count, instrument } => {
            let tok = auth::ensure_token(&cfg).await?;
            let broker = broker::Broker::new(&cfg.api_key, &tok.access_token);
            ratelimit::spam_ltp(broker, &instrument, count).await?;
        }

        Command::ThrottleLtp {
            count,
            sleep_ms,
            instruments,
        } => {
            let tok = auth::ensure_token(&cfg).await?;
            let broker = broker::Broker::new(&cfg.api_key, &tok.access_token);
            ratelimit::throttle_ltp(broker, &instruments, count, sleep_ms).await?;
        }
        Command::History {
            exchange,
            symbol,
            interval,
            days,
            out,
        } => {
            let tok = auth::ensure_token(&cfg).await?;

            let instruments = instruments::load_or_download(
                &cfg.api_key,
                &tok.access_token,
                "instruments_cache.bin",
            )
            .await?;
            let token = instruments::find_instrument_token(&instruments, &exchange, &symbol)
                .with_context(|| format!("instrument_token not found for {exchange}:{symbol}"))?;

            let ist = chrono::FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
            let to_dt = chrono::Local::now().with_timezone(&ist);
            let from_dt = to_dt - chrono::Duration::days(days);

            println!(
                "Fetching {interval} data for {symbol} ({token}) from {from_dt} to {to_dt}..."
            );
            let candles = history::fetch_historical(
                &cfg.api_key,
                &tok.access_token,
                token,
                &interval,
                from_dt,
                to_dt,
                false,
                false,
            )
            .await?;

            history::write_candles_csv(&out, &candles)?;
            println!("Wrote {} candles to {}", candles.len(), out);
        }

        Command::HistoryMtfPlot {
            exchange,
            symbol,
            days,
            out,
        } => {
            let tok = auth::ensure_token(&cfg).await?;

            let instruments = instruments::load_or_download(
                &cfg.api_key,
                &tok.access_token,
                "instruments_cache.bin",
            )
            .await?;
            let token = instruments::find_instrument_token(&instruments, &exchange, &symbol)
                .with_context(|| format!("instrument_token not found for {exchange}:{symbol}"))?;

            history::fetch_mtf_and_plot(&cfg.api_key, &tok.access_token, token, days, &out).await?;
            println!("Saved plot to {}", out);
        }
        Command::NiftyFutOi {
            from,
            to,
            out_csv,
            out_png,
        } => {
            use anyhow::Context;
            use chrono::{FixedOffset, NaiveDate, TimeZone};

            let tok = auth::ensure_token(&cfg).await?;

            // Load instruments from your existing cache (you already added this module earlier)
            let instruments = instruments::load_or_download(
                &cfg.api_key,
                &tok.access_token,
                "instruments_cache.bin",
            )
            .await?;

            // Find front-month NIFTY FUT in NFO
            let front = nfo::find_front_month_nifty_future(&instruments)
                .context("could not find any active NIFTY FUT in NFO")?;

            println!(
                "\n--- Found Live NIFTY Future for Continuous Data ---\ntradingsymbol={} token={} expiry={}",
                front.tradingsymbol, front.instrument_token, front.expiry
            );

            let from_date = NaiveDate::parse_from_str(&from, "%Y-%m-%d")?;
            let to_date = NaiveDate::parse_from_str(&to, "%Y-%m-%d")?;

            let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
            let from_dt = ist
                .from_local_datetime(&from_date.and_hms_opt(0, 0, 0).unwrap())
                .single()
                .unwrap();
            let to_dt = ist
                .from_local_datetime(&to_date.and_hms_opt(23, 59, 59).unwrap())
                .single()
                .unwrap();

            println!(
                "Fetching continuous daily data with OI from {} to {} ...",
                from_dt, to_dt
            );

            // Reuse your existing historical fetch. continuous=true, oi=true are supported. [web:167]
            let candles = history::fetch_historical(
                &cfg.api_key,
                &tok.access_token,
                front.instrument_token,
                "day",
                from_dt,
                to_dt,
                true,
                true,
            )
            .await?;

            history::write_candles_csv(&out_csv, &candles)?;
            println!("Wrote {} candles to {}", candles.len(), out_csv);

            history::plot_close_vs_oi_png(
                &candles,
                &out_png,
                "NIFTY FUT (continuous): Close vs OI",
            )?;
            println!("Saved plot to {}", out_png);
        }
        Command::OrderDemo => {
            use anyhow::Context;
            use tokio::time::{Duration, sleep};

            // Hard safety gate so you don't accidentally place real orders.
            if std::env::var("ORDER_DEMO_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place orders. Set ORDER_DEMO_ENABLED=true in .env to enable OrderDemo."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;

            // Read defaults from .env
            let exchange = std::env::var("ORDER_EXCHANGE").unwrap_or_else(|_| "NSE".to_string());
            let symbol = std::env::var("ORDER_SYMBOL").unwrap_or_else(|_| "INFY".to_string());
            let qty: u32 = std::env::var("ORDER_QTY")
                .unwrap_or_else(|_| "1".to_string())
                .parse()?;
            let product = std::env::var("ORDER_PRODUCT").unwrap_or_else(|_| "CNC".to_string());
            let tag = std::env::var("ORDER_TAG").ok();

            // BUY
            let buy_order_id = orders::place_regular_market_order(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                orders::Side::Buy,
                qty,
                &product,
                tag.as_deref(),
            )
            .await
            .context("BUY order failed")?;

            log::info!("BUY order placed successfully. Order ID: {}", buy_order_id);

            // Small delay (optional)
            sleep(Duration::from_secs(1)).await;

            // SELL
            let sell_order_id = orders::place_regular_market_order(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                orders::Side::Sell,
                qty,
                &product,
                tag.as_deref(),
            )
            .await
            .context("SELL order failed")?;

            log::info!(
                "SELL order placed successfully. Order ID: {}",
                sell_order_id
            );
        }
        Command::OrderLimitManageDemo => {
            env_logger::init();

            // Safety: do not allow live trading unless explicitly enabled.
            if std::env::var("ORDER_LIVE_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place/modify/cancel orders. Set ORDER_LIVE_ENABLED=true in .env to enable."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let broker = broker::Broker::new(&cfg.api_key, &tok.access_token);

            order_flow::run_limit_manage_demo(&cfg.api_key, &tok.access_token, broker).await?;
        }
        Command::Positions => {
            let tok = auth::ensure_token(&cfg).await?;
            let pos = portfolio::fetch_positions(&cfg.api_key, &tok.access_token).await?;
            portfolio::print_positions_table(&pos);
        }

        Command::ConvertMisToCnc => {
            if std::env::var("POSITION_CONVERT_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to convert positions. Set POSITION_CONVERT_ENABLED=true in .env to enable."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;

            let exchange = std::env::var("CONVERT_EXCHANGE").unwrap_or_else(|_| "NSE".to_string());
            let symbol = std::env::var("CONVERT_SYMBOL").unwrap_or_else(|_| "INFY".to_string());
            let qty: u32 = std::env::var("CONVERT_QTY")
                .unwrap_or_else(|_| "1".to_string())
                .parse()?;
            let position_type =
                std::env::var("CONVERT_POSITION_TYPE").unwrap_or_else(|_| "day".to_string());
            let old_product =
                std::env::var("CONVERT_OLD_PRODUCT").unwrap_or_else(|_| "MIS".to_string());
            let new_product =
                std::env::var("CONVERT_NEW_PRODUCT").unwrap_or_else(|_| "CNC".to_string());
            let transaction_type =
                std::env::var("CONVERT_TRANSACTION_TYPE").unwrap_or_else(|_| "BUY".to_string());

            let resp = portfolio::convert_position(
                &cfg.api_key,
                &tok.access_token,
                portfolio::ConvertPositionParams {
                    exchange: &exchange,
                    tradingsymbol: &symbol,
                    transaction_type: &transaction_type,
                    position_type: &position_type,
                    quantity: qty,
                    old_product: &old_product,
                    new_product: &new_product,
                },
            )
            .await?;

            println!("Convert response: {}", resp);
        }
        Command::Holdings => {
            let tok = auth::ensure_token(&cfg).await?;
            let h = holdings::fetch_equity_holdings(&cfg.api_key, &tok.access_token).await?;
            holdings::print_equity_holdings_table(&h);
        }

        Command::HoldingsSummary => {
            let tok = auth::ensure_token(&cfg).await?;
            let h = holdings::fetch_equity_holdings(&cfg.api_key, &tok.access_token).await?;

            let (invested, current, pnl) = holdings::summarize_equity_holdings(&h);
            println!("Total Invested Value: {:.2}", invested);
            println!("Current Market Value: {:.2}", current);
            println!("Total Portfolio P&L: {:.2}", pnl);
        }

        Command::MfHoldings => {
            let tok = auth::ensure_token(&cfg).await?;
            let h = holdings::fetch_mf_holdings(&cfg.api_key, &tok.access_token).await?;
            holdings::print_mf_holdings_table(&h);
        }
        Command::Ticker => {
            let tok = auth::ensure_token(&cfg).await?;

            let tokens_env =
                std::env::var("TICKER_TOKENS").unwrap_or_else(|_| "738561".to_string());
            let mode_env = std::env::var("TICKER_MODE").unwrap_or_else(|_| "full".to_string());

            let tokens = ticker_stream::parse_tokens(&tokens_env)?;
            let mode = mode_env.parse::<ticker_stream::Mode>()?;

            // Run the websocket client on a blocking thread so tokio runtime is fine.
            tokio::task::spawn_blocking(move || {
                ticker_stream::run_ticker_blocking(&cfg.api_key, &tok.access_token, tokens, mode)
            })
            .await??;
        }
        Command::LiveSmaTrader => {
            // Ensure token via your existing non-interactive auth flow
            let tok = auth::ensure_token(&cfg).await?;

            let tokens_env =
                std::env::var("TICKER_TOKENS").unwrap_or_else(|_| "738561".to_string());
            let tokens = ticker_stream::parse_tokens(&tokens_env)?;
            let sma_period: usize = std::env::var("TICKER_SMA_PERIOD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);

            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

            // 1) async order executor
            let api_key = cfg.api_key.clone();
            let access_token = tok.access_token.clone();
            tokio::spawn(async move {
                if let Err(e) = live_sma_trader::run_order_executor(api_key, access_token, rx).await
                {
                    eprintln!("order executor error: {e:?}");
                }
            });

            // 2) blocking websocket tick stream -> signals
            let api_key2 = cfg.api_key.clone();
            let access_token2 = tok.access_token.clone();
            tokio::task::spawn_blocking(move || {
                ticker_stream::run_ticker_signals_blocking(
                    &api_key2,
                    &access_token2,
                    tokens,
                    sma_period,
                    tx,
                )
            })
            .await??;
        }
        Command::PlaceSlm {
            exchange,
            symbol,
            qty,
            trigger,
            product,
        } => {
            // Safety gate so you don't accidentally place real orders
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place orders. Set LIVE_TRADING_ENABLED=true in .env to enable."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let order_id = orders::place_slm_exit_sell(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                qty,
                &product,
                trigger,
                None,
            )
            .await?;

            println!("SL-M order placed. order_id={}", order_id);
        }

        Command::PlaceSl {
            exchange,
            symbol,
            qty,
            trigger,
            price,
            product,
        } => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place orders. Set LIVE_TRADING_ENABLED=true in .env to enable."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let order_id = orders::place_sl_exit_sell(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                qty,
                &product,
                trigger,
                price,
                None,
            )
            .await?;

            println!("SL order placed. order_id={}", order_id);
        }
        Command::LowfreqSma => {
            let tok = auth::ensure_token(&cfg).await?;

            // Reuse instruments cache you already have
            let instruments = instruments::load_or_download(
                &cfg.api_key,
                &tok.access_token,
                "instruments_cache.bin",
            )
            .await?;

            polling_strategy::run_lowfreq_sma_loop(&cfg.api_key, &tok.access_token, &instruments)
                .await?;
        }
        Command::CoDemo => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place live orders. Set LIVE_TRADING_ENABLED=true in .env."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let exchange = std::env::var("ADV_EXCHANGE").unwrap_or_else(|_| "NSE".into());
            let product = std::env::var("ADV_PRODUCT").unwrap_or_else(|_| "MIS".into());
            let symbol = std::env::var("CO_SYMBOL").unwrap_or_else(|_| "TATAMOTORS".into());
            let qty: u32 = std::env::var("CO_QTY")
                .unwrap_or_else(|_| "10".into())
                .parse()?;
            let sl_trigger: f64 = std::env::var("CO_SL_TRIGGER")
                .unwrap_or_else(|_| "650".into())
                .parse()?;
            let tag = std::env::var("ADV_TAG").ok();

            let order_id = orders::place_cover_order_market_buy(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                qty,
                &product,
                sl_trigger,
                tag.as_deref(),
            )
            .await?;

            println!("CO placed. order_id={}", order_id);
        }

        Command::AmoDemo => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place live orders. Set LIVE_TRADING_ENABLED=true in .env."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let exchange = std::env::var("ADV_EXCHANGE").unwrap_or_else(|_| "NSE".into());
            let symbol = std::env::var("AMO_SYMBOL").unwrap_or_else(|_| "INFY".into());
            let qty: u32 = std::env::var("AMO_QTY")
                .unwrap_or_else(|_| "5".into())
                .parse()?;
            let price: f64 = std::env::var("AMO_LIMIT_PRICE")
                .unwrap_or_else(|_| "1600".into())
                .parse()?;
            let product = std::env::var("AMO_PRODUCT").unwrap_or_else(|_| "CNC".into());
            let tag = std::env::var("ADV_TAG").ok();

            let order_id = orders::place_amo_limit_order(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                orders::Side::Buy,
                qty,
                &product,
                price,
                tag.as_deref(),
            )
            .await?;

            println!("AMO placed. order_id={}", order_id);
        }

        Command::IcebergDemo => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place live orders. Set LIVE_TRADING_ENABLED=true in .env."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let exchange = std::env::var("ADV_EXCHANGE").unwrap_or_else(|_| "NSE".into());
            let symbol = std::env::var("ICE_SYMBOL").unwrap_or_else(|_| "ITC".into());
            let total_qty: u32 = std::env::var("ICE_QTY")
                .unwrap_or_else(|_| "2000".into())
                .parse()?;
            let legs: u32 = std::env::var("ICE_LEGS")
                .unwrap_or_else(|_| "4".into())
                .parse()?;
            let limit_price: f64 = std::env::var("ICE_LIMIT_PRICE")
                .unwrap_or_else(|_| "435".into())
                .parse()?;
            let product = std::env::var("ICE_PRODUCT").unwrap_or_else(|_| "CNC".into());
            let tag = std::env::var("ADV_TAG").ok();

            let iceberg_qty = total_qty / legs; // keep it simple like your notebook
            let order_id = orders::place_iceberg_limit_buy(
                &cfg.api_key,
                &tok.access_token,
                &exchange,
                &symbol,
                total_qty,
                &product,
                limit_price,
                legs,
                iceberg_qty,
                tag.as_deref(),
            )
            .await?;

            println!("Iceberg placed. parent order_id={}", order_id);
        }

        Command::GttSingleDemo => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place live orders. Set LIVE_TRADING_ENABLED=true in .env."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;

            let exchange = std::env::var("ADV_EXCHANGE").unwrap_or_else(|_| "NSE".into());
            let symbol = std::env::var("GTT_SINGLE_SYMBOL").unwrap_or_else(|_| "TCS".into());
            let qty: u32 = std::env::var("GTT_SINGLE_QTY")
                .unwrap_or_else(|_| "5".into())
                .parse()?;
            let trigger: f64 = std::env::var("GTT_SINGLE_TRIGGER")
                .unwrap_or_else(|_| "3000".into())
                .parse()?;
            let order_price: f64 = std::env::var("GTT_SINGLE_ORDER_PRICE")
                .unwrap_or_else(|_| "3001".into())
                .parse()?;
            let product = std::env::var("GTT_SINGLE_PRODUCT").unwrap_or_else(|_| "CNC".into());

            // Reuse your existing broker.ltp() implementation to get last_price.
            let mut broker = broker::Broker::new(&cfg.api_key, &tok.access_token);

            let key = format!("{exchange}:{symbol}");
            let ltp_json = broker.ltp(&[key.as_str()])?;

            // let ltp_json = broker.ltp(&[key.as_str()])?;
            let last_price = ltp_json["data"][&key]["last_price"].as_f64().unwrap_or(0.0);

            let condition = gtt::GttCondition {
                exchange: &exchange,
                tradingsymbol: &symbol,
                trigger_values: vec![trigger],
                last_price,
            };

            let orders = [gtt::GttOrder {
                exchange: &exchange,
                tradingsymbol: &symbol,
                transaction_type: "BUY",
                quantity: qty,
                order_type: "LIMIT",
                product: &product,
                price: order_price,
            }];

            let trigger_id = gtt::place_gtt(
                &cfg.api_key,
                &tok.access_token,
                "single",
                &condition,
                &orders,
            )
            .await?;
            println!("GTT single placed. trigger_id={}", trigger_id);
        }

        Command::GttOcoDemo => {
            if std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() != "true" {
                anyhow::bail!(
                    "Refusing to place live orders. Set LIVE_TRADING_ENABLED=true in .env."
                );
            }

            let tok = auth::ensure_token(&cfg).await?;
            let exchange = std::env::var("ADV_EXCHANGE").unwrap_or_else(|_| "NSE".into());
            let symbol = std::env::var("GTT_OCO_SYMBOL").unwrap_or_else(|_| "HDFCBANK".into());
            let qty: u32 = std::env::var("GTT_OCO_QTY")
                .unwrap_or_else(|_| "5".into())
                .parse()?;
            let sl_trigger: f64 = std::env::var("GTT_OCO_SL_TRIGGER")
                .unwrap_or_else(|_| "1700".into())
                .parse()?;
            let tgt_trigger: f64 = std::env::var("GTT_OCO_TARGET_TRIGGER")
                .unwrap_or_else(|_| "2300".into())
                .parse()?;
            let sl_price: f64 = std::env::var("GTT_OCO_SL_ORDER_PRICE")
                .unwrap_or_else(|_| "1700".into())
                .parse()?;
            let tgt_price: f64 = std::env::var("GTT_OCO_TARGET_ORDER_PRICE")
                .unwrap_or_else(|_| "2300".into())
                .parse()?;
            let product = std::env::var("GTT_OCO_PRODUCT").unwrap_or_else(|_| "CNC".into());
            let mut broker = broker::Broker::new(&cfg.api_key, &tok.access_token);

            let key = format!("{exchange}:{symbol}");
            let ltp_json = broker.ltp(&[key.as_str()])?;

            let last_price = ltp_json["data"][&key]["last_price"].as_f64().unwrap_or(0.0);

            let condition = gtt::GttCondition {
                exchange: &exchange,
                tradingsymbol: &symbol,
                trigger_values: vec![sl_trigger, tgt_trigger],
                last_price,
            };

            let orders = [
                gtt::GttOrder {
                    exchange: &exchange,
                    tradingsymbol: &symbol,
                    transaction_type: "SELL",
                    quantity: qty,
                    order_type: "LIMIT",
                    product: &product,
                    price: tgt_price,
                },
                gtt::GttOrder {
                    exchange: &exchange,
                    tradingsymbol: &symbol,
                    transaction_type: "SELL",
                    quantity: qty,
                    order_type: "LIMIT",
                    product: &product,
                    price: sl_price,
                },
            ];

            let trigger_id = gtt::place_gtt(
                &cfg.api_key,
                &tok.access_token,
                "two-leg",
                &condition,
                &orders,
            )
            .await?;
            println!("GTT OCO placed. trigger_id={}", trigger_id);
        }
        Command::BacktestSma => {
            let tok = auth::ensure_token(&cfg).await?;
            backtest_sma::run_backtest_sma(&cfg.api_key, &tok.access_token).await?;
        }
    }

    Ok(())
}
