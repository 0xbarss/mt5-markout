use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use markout::{
    event_bus::{EventBus, MarketEvent},
    ingestion::Dataset,
    models::{Bar as MarkoutBar, Tick as MarkoutTick},
    Config, Mode,
};
use mt5_bridge::{Mt5Client, StreamMode, Timeframe};
use tracing::{error, info, warn};

/// Real-time MetaTrader 5 bridge and Markout live web charting monitor.
#[derive(Debug, Parser)]
#[command(
    name = "mt5-markout",
    version = "0.1.0",
    about = "Streams live MT5 bars and ticks into Markout web visualizer"
)]
struct Args {
    /// Symbol to monitor (e.g. EURUSD, GBPUSD, BTCUSD).
    #[arg(short, long, default_value = "EURUSD")]
    symbol: String,

    /// Chart timeframe: M1, M5, M15, M30, H1, H4, D1, etc.
    #[arg(short, long, default_value = "M1")]
    tf: String,

    /// Number of initial historical bars to preload into Markout.
    #[arg(long, default_value_t = 300)]
    history_bars: usize,

    /// MetaTrader 5 Expert Advisor IPC pipe secret.
    #[arg(long, env = "MT5_PIPE_SECRET", default_value = "test")]
    secret: String,

    /// MetaTrader 5 account login number (optional, 0 for secret-only auth).
    #[arg(long, env = "MT5_LOGIN", default_value_t = 0)]
    login: i64,

    /// MetaTrader 5 broker server name (optional).
    #[arg(long, env = "MT5_SERVER", default_value = "")]
    server: String,

    /// Web server host interface to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Web server port to listen on.
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Optional explicit path to mt5_bridge.dll.
    #[arg(long, env = "MT5_DLL_PATH")]
    dll_path: Option<PathBuf>,

    /// Bar refresh/polling interval in milliseconds.
    #[arg(long, default_value_t = 500)]
    poll_interval_ms: u64,

    /// Disable streaming ticks (only stream bars).
    #[arg(long)]
    no_ticks: bool,

    /// Disable streaming bars (only stream ticks).
    #[arg(long)]
    no_bars: bool,
}

/// Resolves the mt5_bridge.dll location using multiple candidate directories.
fn resolve_dll_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        if path.exists() {
            return path.to_path_buf();
        }
    }

    if let Some(env_path) = std::env::var_os("MT5_DLL_PATH") {
        let pb = PathBuf::from(env_path);
        if pb.exists() {
            return pb;
        }
    }

    // Check adjacent to running executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("mt5_bridge.dll");
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    // Check current working directory
    let candidate = PathBuf::from("mt5_bridge.dll");
    if candidate.is_file() {
        return candidate;
    }

    // Check standard Wine MT5 installation path
    let wine_mt5_dll = PathBuf::from(
        r"C:\Program Files\MetaTrader 5\MQL5\Libraries\mt5_bridge.dll",
    );
    if wine_mt5_dll.is_file() {
        return wine_mt5_dll;
    }

    // Fallback default name
    PathBuf::from("mt5_bridge.dll")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mt5_markout=info,markout=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();
    let timeframe: Timeframe = args
        .tf
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid timeframe '{}': {}", args.tf, e))?;

    let dll_path = resolve_dll_path(args.dll_path.as_deref());
    info!(
        "Connecting to MetaTrader 5 EA via IPC (DLL: {}, Secret: '***')...",
        dll_path.display()
    );

    let client = Arc::new(
        Mt5Client::connect_with_dll(&dll_path, args.login, &args.secret, &args.server)
            .with_context(|| {
                format!(
                    "Failed to connect to MT5 bridge using DLL '{}'. Verify Wine MetaTrader 5 is running with mt5_bridge EA attached.",
                    dll_path.display()
                )
            })?,
    );

    // Query and display account info
    match client.account_info() {
        Ok(acct) => {
            info!(
                "Connected to MT5 Account | Balance: {:.2} | Equity: {:.2} | Free Margin: {:.2}",
                acct.balance, acct.equity, acct.free_margin
            );
        }
        Err(e) => {
            warn!("Could not fetch initial account info: {}", e);
        }
    }

    // Query and display symbol info
    match client.symbol_info(&args.symbol) {
        Ok(info) => {
            info!(
                "Symbol {} specs | Digits: {} | Point: {:.5} | Spread: {:.1} pts",
                info.symbol, info.digits, info.point, info.spread
            );
        }
        Err(e) => {
            warn!("Could not fetch symbol specs for {}: {}", args.symbol, e);
        }
    }

    // Preload historical bars
    let now = Utc::now().timestamp();
    let lookback_sec = (args.history_bars as i64) * timeframe.seconds() * 2;
    let from_time = (now - lookback_sec).max(0);

    info!(
        "Preloading up to {} historical {} bars for {}...",
        args.history_bars, timeframe, args.symbol
    );

    let initial_bars = match client.copy_bars(&args.symbol, timeframe, from_time, now) {
        Ok(bars) => {
            let count = bars.len();
            let markout_bars: Vec<MarkoutBar> = bars
                .into_iter()
                .map(|b| MarkoutBar {
                    time: b.time,
                    open: b.open,
                    high: b.high,
                    low: b.low,
                    close: b.close,
                    volume: b.volume,
                })
                .collect();
            let slice_start = markout_bars.len().saturating_sub(args.history_bars);
            let trimmed = markout_bars[slice_start..].to_vec();
            info!(
                "Loaded {} historical bars (showing latest {})",
                count,
                trimmed.len()
            );
            trimmed
        }
        Err(e) => {
            warn!("Could not preload historical bars: {}. Starting empty.", e);
            Vec::new()
        }
    };

    let dataset = Dataset {
        bars: initial_bars,
        trades: Vec::new(),
        signals: Vec::new(),
    };

    let bus = EventBus::new(4096);

    // Spawn Tick Stream
    if !args.no_ticks {
        let client_tick = Arc::clone(&client);
        let bus_tick = bus.clone();
        let sym_tick = args.symbol.clone();

        tokio::spawn(async move {
            info!("Starting real-time push tick feed for {}...", sym_tick);
            match client_tick.subscribe_ticks_with_mode(&sym_tick, StreamMode::Latest) {
                Ok(mut sub) => {
                    while let Some(tick) = sub.recv().await {
                        let price = if tick.last > 0.0 {
                            tick.last
                        } else if tick.bid > 0.0 {
                            tick.bid
                        } else {
                            tick.ask
                        };

                        let m_tick = MarkoutTick {
                            symbol: tick.symbol,
                            time: tick.time,
                            price,
                            bid: Some(tick.bid),
                            ask: Some(tick.ask),
                        };

                        bus_tick.publish(MarketEvent::Tick(m_tick));
                    }
                    warn!("Tick push stream ended for {}", sym_tick);
                }
                Err(e) => {
                    warn!(
                        "Could not subscribe to push ticks: {}. Falling back to polling stream.",
                        e
                    );
                    let mut rx = mt5_bridge::stream_ticks(
                        client_tick,
                        &sym_tick,
                        Duration::from_millis(100),
                    );
                    while let Some(tick) = rx.recv().await {
                        let price = if tick.last > 0.0 {
                            tick.last
                        } else if tick.bid > 0.0 {
                            tick.bid
                        } else {
                            tick.ask
                        };
                        let m_tick = MarkoutTick {
                            symbol: tick.symbol,
                            time: tick.time,
                            price,
                            bid: Some(tick.bid),
                            ask: Some(tick.ask),
                        };
                        bus_tick.publish(MarketEvent::Tick(m_tick));
                    }
                }
            }
        });
    }

    // Spawn Live Bar Stream
    if !args.no_bars {
        let client_bars = Arc::clone(&client);
        let bus_bars = bus.clone();
        let sym_bar = args.symbol.clone();
        let poll_interval = Duration::from_millis(args.poll_interval_ms);

        tokio::spawn(async move {
            info!(
                "Starting live bar polling stream for {} {} (every {} ms)...",
                sym_bar,
                timeframe,
                poll_interval.as_millis()
            );

            let mut last_emitted_time = 0i64;
            let mut last_emitted_close = 0.0f64;
            let mut last_emitted_vol = 0.0f64;

            loop {
                tokio::time::sleep(poll_interval).await;
                let now = Utc::now().timestamp();
                let from = (now - timeframe.seconds() * 10).max(0);
                let to = now + timeframe.seconds() * 2;

                let client_task = Arc::clone(&client_bars);
                let sym_task = sym_bar.clone();

                let rates_res = tokio::task::spawn_blocking(move || {
                    client_task.copy_rates(&sym_task, timeframe, from, to)
                })
                .await;

                match rates_res {
                    Ok(Ok(rates)) => {
                        if rates.len() >= 2 {
                            let closed = &rates[rates.len() - 2];
                            let forming = &rates[rates.len() - 1];

                            let is_new_time = forming.time != last_emitted_time;
                            if is_new_time {
                                // Emit finalized closed candle first so previous candle is accurate
                                let m_closed = MarkoutBar {
                                    time: closed.time,
                                    open: closed.open,
                                    high: closed.high,
                                    low: closed.low,
                                    close: closed.close,
                                    volume: closed.volume as f64,
                                };
                                bus_bars.publish(MarketEvent::Bar(m_closed));
                            }

                            let is_updated_price = (forming.close - last_emitted_close).abs() > 1e-9;
                            let is_updated_vol = (forming.volume as f64 - last_emitted_vol).abs() > 1e-9;

                            if is_new_time || is_updated_price || is_updated_vol {
                                last_emitted_time = forming.time;
                                last_emitted_close = forming.close;
                                last_emitted_vol = forming.volume as f64;

                                let m_bar = MarkoutBar {
                                    time: forming.time,
                                    open: forming.open,
                                    high: forming.high,
                                    low: forming.low,
                                    close: forming.close,
                                    volume: forming.volume as f64,
                                };
                                bus_bars.publish(MarketEvent::Bar(m_bar));
                            }
                        } else if let Some(forming) = rates.last() {
                            let is_new_time = forming.time != last_emitted_time;
                            let is_updated_price = (forming.close - last_emitted_close).abs() > 1e-9;
                            let is_updated_vol = (forming.volume as f64 - last_emitted_vol).abs() > 1e-9;

                            if is_new_time || is_updated_price || is_updated_vol {
                                last_emitted_time = forming.time;
                                last_emitted_close = forming.close;
                                last_emitted_vol = forming.volume as f64;

                                let m_bar = MarkoutBar {
                                    time: forming.time,
                                    open: forming.open,
                                    high: forming.high,
                                    low: forming.low,
                                    close: forming.close,
                                    volume: forming.volume as f64,
                                };
                                bus_bars.publish(MarketEvent::Bar(m_bar));
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        warn!("Error polling rates: {}", e);
                    }
                    Err(e) => {
                        error!("Polling task panicked: {}", e);
                        break;
                    }
                }
            }
        });
    }

    let config = Config {
        host: args.host.clone(),
        port: args.port,
        mode: Mode::Live {
            feed: Some("mt5-bridge".to_string()),
            symbol: Some(args.symbol.clone()),
            tf: Some(args.tf.clone()),
        },
    };

    println!("\n========================================================");
    println!("   MT5 -> Markout Live Streaming Monitor Running");
    println!("   Symbol:    {}", args.symbol);
    println!("   Timeframe: {}", args.tf);
    println!("   Web UI:    http://{}:{}", args.host, args.port);
    println!("========================================================\n");

    markout::serve_with_data(config, bus, dataset).await?;

    Ok(())
}
