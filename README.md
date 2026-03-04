# Zerodha Kite CLI Wrapper (Rust)

This project is a **CLI wrapper over Zerodha Kite Connect APIs**.

Run commands with Cargo like:

```bash
cargo run -- <subcommand> [flags...]
```

Everything after `--` is passed to your binary.

---

# How to Run

### Show all commands

```bash
cargo run -- --help
```

(Cargo passes arguments after `--` to the program)

### Show help for one command

```bash
cargo run -- history --help
```

Many **danger commands require environment safety gates** like:

```
LIVE_TRADING_ENABLED=true
ORDER_LIVE_ENABLED=true
```

This prevents placing **real trades by accident**.

---

# Auth & Session Commands

These commands manage login/session and read user profile/margins.

| Command | Run                    | Plain-English Meaning                                                                                  | Kite API Used                                                                              |
| ------- | ---------------------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| Auth    | `cargo run -- auth`    | Starts login flow, stores access token, then verifies by fetching your profile                         | `POST /session/token`, `GET /user/profile`                                                 |
| Logout  | `cargo run -- logout`  | Logs out and invalidates current API session/token and deletes local token file                        | `DELETE /session/token`                                                                    |
| Run     | `cargo run -- run`     | Startup demo: loads instruments, prints LTP/OHLC/Quote snapshots, prints profile, then starts bot loop | `GET /instruments`, `GET /quote/ltp`, `GET /quote/ohlc`, `GET /quote`, `GET /user/profile` |
| Margins | `cargo run -- margins` | Prints available funds/margins                                                                         | `GET /user/margins/:segment`                                                               |

---

# Market Data & Historical Data

These commands fetch quotes, stream ticks, or download historical candles.

| Command        | Run                                                                                                   | Meaning                                                      | Kite API                                                  |
| -------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------------------------------------- |
| SpamLtp        | `cargo run -- spam-ltp --count 200 --instrument NSE:INFY`                                             | Spams LTP requests to demonstrate rate limits                | `GET /quote/ltp`                                          |
| ThrottleLtp    | `cargo run -- throttle-ltp --count 5 --sleep-ms 1100 --instruments NSE:INFY,NSE:RELIANCE`             | Fetches LTP slowly in batches (recommended)                  | `GET /quote/ltp`                                          |
| Ticker         | `cargo run -- ticker`                                                                                 | Opens WebSocket and prints live ticks                        | WebSocket streaming                                       |
| History        | `cargo run -- history --exchange NSE --symbol INFY --interval day --days 90 --out historical.csv`     | Downloads historical candles and saves CSV                   | `GET /instruments/historical/:instrument_token/:interval` |
| HistoryMtfPlot | `cargo run -- history-mtf-plot --exchange NSE --symbol INFY --days 5 --out mtf.png`                   | Downloads multiple timeframes and generates plot             | Historical candles endpoint                               |
| NiftyFutOi     | `cargo run -- nifty-fut-oi --from 2024-05-01 --to 2024-05-31 --out-csv nifty.csv --out-png nifty.png` | Downloads front-month NIFTY future candles with OI and plots | Historical endpoint with `continuous=1` and `oi=1`        |
| LowfreqSma     | `cargo run -- lowfreq-sma`                                                                            | Polling strategy computing SMA(200) signals                  | Historical candles endpoint                               |

---

# Portfolio & Holdings

These commands read portfolio/positions/holdings.

| Command         | Run                             | Meaning                                                 | Kite API                       |
| --------------- | ------------------------------- | ------------------------------------------------------- | ------------------------------ |
| Positions       | `cargo run -- positions`        | Prints open positions                                   | Portfolio positions endpoint   |
| Holdings        | `cargo run -- holdings`         | Prints equity holdings                                  | Portfolio holdings endpoint    |
| HoldingsSummary | `cargo run -- holdings-summary` | Calculates invested value, current value, and total P&L | Holdings endpoint + local math |
| MfHoldings      | `cargo run -- mf-holdings`      | Prints mutual fund holdings                             | `/mf/holdings` endpoint        |

---

# Trading, Orders, GTT and Danger Commands

These commands **place, modify or cancel orders**, so safety flags must be enabled.

---

# A) Live Strategy Execution

| Command         | Run                               | Meaning                                                           | Kite API                           |
| --------------- | --------------------------------- | ----------------------------------------------------------------- | ---------------------------------- |
| LiveSmaTrader   | `cargo run -- live-sma-trader`    | Streams ticks, generates SMA signals and optionally places orders | WebSocket + `POST /orders/regular` |
| ConvertMisToCnc | `cargo run -- convert-mis-to-cnc` | Converts position type (MIS → CNC)                                | Portfolio conversion endpoint      |

---

# B) Simple Order Demos

| Command              | Run                                    | Meaning                                            | Kite API               |
| -------------------- | -------------------------------------- | -------------------------------------------------- | ---------------------- |
| OrderDemo            | `cargo run -- order-demo`              | Places MARKET BUY then MARKET SELL                 | `POST /orders/regular` |
| OrderLimitManageDemo | `cargo run -- order-limit-manage-demo` | Place LIMIT order, modify, cancel and view history | Orders API             |

---

# C) Stop Loss Orders

| Command  | Run                                                                                                       | Meaning                     | Kite API          |
| -------- | --------------------------------------------------------------------------------------------------------- | --------------------------- | ----------------- |
| PlaceSlm | `cargo run -- place-slm --exchange NSE --symbol INFY --qty 10 --trigger 1540 --product MIS`               | Stop-loss market exit order | `order_type=SL-M` |
| PlaceSl  | `cargo run -- place-sl --exchange NSE --symbol INFY --qty 10 --trigger 1540 --price 1539.5 --product MIS` | Stop-loss limit exit order  | `order_type=SL`   |

---

# D) Advanced Order Varieties

| Command     | Run                         | Meaning                              | Kite API               |
| ----------- | --------------------------- | ------------------------------------ | ---------------------- |
| CoDemo      | `cargo run -- co-demo`      | Places Cover Order                   | `POST /orders/co`      |
| AmoDemo     | `cargo run -- amo-demo`     | Places After Market Order            | `POST /orders/amo`     |
| IcebergDemo | `cargo run -- iceberg-demo` | Splits large order into smaller legs | `POST /orders/iceberg` |

---

# E) GTT Triggers (Good Till Triggered)

| Command       | Run                            | Meaning                   | Kite API                          |
| ------------- | ------------------------------ | ------------------------- | --------------------------------- |
| GttSingleDemo | `cargo run -- gtt-single-demo` | Single trigger GTT        | `POST /gtt/triggers type=single`  |
| GttOcoDemo    | `cargo run -- gtt-oco-demo`    | OCO GTT (two-leg trigger) | `POST /gtt/triggers type=two-leg` |

---

# F) Margin Estimation

| Command      | Run          | Meaning                                                    | Kite API               |
| ------------ | ------------ | ---------------------------------------------------------- | ---------------------- |
| Margin Check | _(internal)_ | Calculates margin required for an order without placing it | `POST /margins/orders` |

---

# Quick Run Checklist

## Safe (Read-only) Commands

```bash
cargo run -- auth
cargo run -- run
cargo run -- margins
cargo run -- positions
cargo run -- holdings
cargo run -- holdings-summary
cargo run -- mf-holdings
cargo run -- history --exchange NSE --symbol INFY --interval day --days 30 --out infy_30d.csv
cargo run -- lowfreq-sma
cargo run -- ticker
```

These use **quote snapshots and historical data endpoints**.

---

## Danger Commands (Place Orders)

Enable environment safety flags before running.

```bash
cargo run -- order-demo
cargo run -- order-limit-manage-demo
cargo run -- place-slm --trigger 1540
cargo run -- place-sl --trigger 1540 --price 1539.5
cargo run -- co-demo
cargo run -- amo-demo
cargo run -- iceberg-demo
cargo run -- gtt-single-demo
cargo run -- gtt-oco-demo
cargo run -- live-sma-trader
cargo run -- convert-mis-to-cnc
```

These rely on **Kite Orders, GTT, and Portfolio APIs**.
