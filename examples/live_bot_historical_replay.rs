//! Historical replay of the production `LiveBot::process_bar()` path.
//!
//! Purpose: validate the live event/state-machine path against cached daily bars
//! without API keys or WebSocket dependencies. This is intentionally NOT the
//! source-of-truth compounding equity harness (`live_bot_exact_equity.rs`); it is
//! a deployability check that the production bot can consume historical bars in
//! configured symbol order and generate closed trades in dry-run mode.
//!
//! Outputs:
//! - snapshots/live_bot_historical_replay.md

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use krypto::data::loader::DataLoader;
use krypto::live::bot::LiveBot;
use krypto::live::config::LiveConfig;
use krypto::live::feed::{KlineData, KlineEvent};
use krypto::paper::Bar;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

fn bar_to_event(symbol: &str, interval: &str, bar: &Bar) -> KlineEvent {
    let start_time = bar.time.timestamp_millis();
    let end_time = start_time + 86_400_000 - 1;
    KlineEvent {
        event_type: "kline".to_string(),
        event_time: end_time,
        symbol: symbol.to_string(),
        kline: KlineData {
            start_time,
            end_time,
            symbol: symbol.to_string(),
            interval: interval.to_string(),
            open: bar.open.to_string(),
            close: bar.close.to_string(),
            high: bar.high.to_string(),
            low: bar.low.to_string(),
            volume: bar.volume.to_string(),
            is_closed: true,
        },
    }
}

async fn load_bars(loader: &DataLoader, symbol: &str) -> Result<Vec<Bar>> {
    let df = loader.fetch_data(symbol, "1d", CANDLES).await?;
    let time = df.column("time")?.datetime()?;
    let open = df.column("open")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let t = time.get(i).context("missing timestamp")?;
        let dt = DateTime::<Utc>::from_timestamp_millis(t).context("invalid timestamp")?;
        bars.push(Bar::new(
            dt,
            open.get(i).context("missing open")?,
            high.get(i).context("missing high")?,
            low.get(i).context("missing low")?,
            close.get(i).context("missing close")?,
            volume.get(i).context("missing volume")?,
        ));
    }
    Ok(bars)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== LIVE BOT HISTORICAL REPLAY ===");
    println!(
        "Purpose: feed cached daily bars through src/live/bot.rs::process_bar() in dry-run mode"
    );

    let loader = DataLoader::new(None, None);
    let mut raw: HashMap<String, Vec<Bar>> = HashMap::new();
    for sym in BASE_SYMBOLS {
        let bars = load_bars(&loader, sym).await?;
        println!("Loaded {:>7}: {} bars", sym, bars.len());
        raw.insert(sym.to_string(), bars);
    }

    // Align by exact daily timestamp so one replay index represents one UTC day.
    let mut common: HashSet<i64> = raw
        .get("BTCUSDT")
        .context("BTCUSDT loaded")?
        .iter()
        .map(|b| b.time.timestamp_millis())
        .collect();
    for bars in raw.values() {
        let times: HashSet<i64> = bars.iter().map(|b| b.time.timestamp_millis()).collect();
        common = common.intersection(&times).copied().collect();
    }
    let mut common_times: Vec<i64> = common.into_iter().collect();
    common_times.sort_unstable();
    anyhow::ensure!(
        common_times.len() > WARMUP_BARS + 1,
        "not enough common bars for replay"
    );

    let mut aligned: HashMap<String, Vec<Bar>> = HashMap::new();
    for sym in BASE_SYMBOLS {
        let by_time: HashMap<i64, Bar> = raw
            .get(sym)
            .unwrap()
            .iter()
            .cloned()
            .map(|b| (b.time.timestamp_millis(), b))
            .collect();
        let bars = common_times
            .iter()
            .filter_map(|t| by_time.get(t).cloned())
            .collect::<Vec<_>>();
        aligned.insert(sym.to_string(), bars);
    }

    let mut config = LiveConfig::default();
    config.symbols = BASE_SYMBOLS.iter().map(|s| s.to_string()).collect();
    config.interval = "1d".to_string();
    config.dry_run = true;
    config.use_testnet = true;

    let mut bot = LiveBot::new(config.clone())?;
    for sym in BASE_SYMBOLS {
        let seed = aligned.get(sym).unwrap()[..WARMUP_BARS].to_vec();
        bot.seed_history(sym.to_string(), seed);
    }

    let mut processed_events = 0usize;
    for idx in WARMUP_BARS..common_times.len() {
        for sym in &config.symbols {
            let bar = &aligned.get(sym).unwrap()[idx];
            let event = bar_to_event(sym, &config.interval, bar);
            bot.process_bar(&event).await?;
            processed_events += 1;
        }
    }

    let state = bot.state().await;
    let first_date = DateTime::<Utc>::from_timestamp_millis(common_times[WARMUP_BARS]).unwrap();
    let last_date = DateTime::<Utc>::from_timestamp_millis(*common_times.last().unwrap()).unwrap();

    fs::create_dir_all("snapshots")?;
    let mut md = File::create("snapshots/live_bot_historical_replay.md")?;
    writeln!(md, "# Live Bot Historical Replay")?;
    writeln!(md)?;
    writeln!(
        md,
        "Production `LiveBot::process_bar()` replay against cached aligned daily bars."
    )?;
    writeln!(md)?;
    writeln!(md, "| Metric | Value |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(md, "| Symbols | {} |", config.symbols.join(", "))?;
    writeln!(md, "| Common aligned days | {} |", common_times.len())?;
    writeln!(md, "| Warmup bars seeded per symbol | {} |", WARMUP_BARS)?;
    writeln!(
        md,
        "| Replayed date range | {} → {} |",
        first_date.date_naive(),
        last_date.date_naive()
    )?;
    writeln!(md, "| Processed closed-bar events | {} |", processed_events)?;
    writeln!(
        md,
        "| Closed trades recorded by LiveBot | {} |",
        bot.completed_trade_count()
    )?;
    writeln!(md, "| State trades | {} |", state.trades)?;
    writeln!(
        md,
        "| Open positions at end | {} |",
        bot.open_position_count()
    )?;
    writeln!(md, "| Dry run | {} |", config.dry_run)?;
    writeln!(md)?;
    writeln!(md, "## Interpretation")?;
    writeln!(md)?;
    writeln!(md, "This harness closes the no-API replay gap at the production event path level: cached bars can now drive `src/live/bot.rs` directly without WebSocket/testnet credentials.")?;
    writeln!(md)?;
    writeln!(md, "It is **not** a replacement for `live_bot_exact_equity.rs`: live `BotState.equity` is operational monitoring state, not the compounding account-equity source of truth. Use this replay to catch production path breakage; use exact-live equity for performance metrics.")?;

    println!("Processed events: {}", processed_events);
    println!("Closed trades: {}", bot.completed_trade_count());
    println!("Open positions at end: {}", bot.open_position_count());
    println!("Wrote snapshots/live_bot_historical_replay.md");

    Ok(())
}
