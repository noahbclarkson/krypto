//! Intraday OI + funding crowding-state study.
//!
//! Purpose:
//! - reopen the derivatives-state lane honestly at the only public horizon that looks remotely usable
//! - treat Binance public OI as a short-horizon exploratory input, not a multi-year daily filter
//! - test whether simple crowding states have directional value over the next 24 hours
//!
//! Lens:
//! - 1h bars
//! - latest public Binance OI window only (~20 days in practice)
//! - funding aligned from 8h updates
//! - evaluate next-24h close-to-close return from state at bar close
//!
//! This is state evidence only, not a deployable strategy benchmark.

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::{
    align_funding_to_ohlcv, align_open_interest_to_ohlcv, DataLoader, FundingRateLoader,
    OpenInterestLoader, OpenInterestPeriod,
};
use polars::prelude::*;
use std::cmp::Ordering;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT",
];
const CANDLES_1H: u32 = 1200;
const HOLD_BARS: usize = 24;
const PRICE_LOOKBACK: usize = 24;
const OI_WINDOW: usize = 72; // 72h rolling baseline on 1h OI
const FUNDING_Z_WINDOW: usize = 60; // 60 funding periods ~= 20 days

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StateKind {
    HealthyLong,
    CrowdedLong,
    ShortSqueeze,
    HealthyFlush,
    CrowdedShort,
    Capitulation,
    Other,
}

impl StateKind {
    fn all() -> &'static [StateKind] {
        &[
            Self::HealthyLong,
            Self::CrowdedLong,
            Self::ShortSqueeze,
            Self::HealthyFlush,
            Self::CrowdedShort,
            Self::Capitulation,
            Self::Other,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::HealthyLong => "HealthyLong",
            Self::CrowdedLong => "CrowdedLong",
            Self::ShortSqueeze => "ShortSqueeze",
            Self::HealthyFlush => "HealthyFlush",
            Self::CrowdedShort => "CrowdedShort",
            Self::Capitulation => "Capitulation",
            Self::Other => "Other",
        }
    }
}

#[derive(Default, Clone, Debug)]
struct StatLine {
    count: usize,
    wins: usize,
    sum_ret: f64,
}

impl StatLine {
    fn add(&mut self, r: f64) {
        self.count += 1;
        if r > 0.0 {
            self.wins += 1;
        }
        self.sum_ret += r;
    }
    fn avg_ret_pct(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum_ret / self.count as f64 * 100.0
        }
    }
    fn win_rate_pct(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.wins as f64 / self.count as f64 * 100.0
        }
    }
}

#[derive(Clone)]
struct AssetRows {
    time: Vec<i64>,
    close: Vec<f64>,
    funding_z: Vec<f64>,
    oi_change_1: Vec<f64>,
    oi_change_7: Vec<f64>,
    oi_z: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== INTRADAY OI + FUNDING CROWDING STUDY ===\n");
    println!(
        "Lens: 1h bars, state at close, next {}h close-to-close return",
        HOLD_BARS
    );
    println!("Trust note: this uses only the latest public Binance OI window, so it is exploratory context, not chronology-grade evidence.\n");

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");
    let oi_loader = OpenInterestLoader::with_cache_dir("examples/open_interest_cache");

    let mut assets = HashMap::<String, AssetRows>::new();

    for &symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1h", CANDLES_1H).await?;
        let funding = funding_loader.fetch(symbol, None, None).await?;
        let oi = oi_loader
            .fetch_with_period(symbol, OpenInterestPeriod::H1, None, None)
            .await?;
        let with_funding = align_funding_to_ohlcv(&raw, &funding, FUNDING_Z_WINDOW)?;
        let with_state = align_open_interest_to_ohlcv(&with_funding, &oi, OI_WINDOW)?;
        let rows = dataframe_to_rows(&with_state)?;
        if let (Some(first), Some(last)) = (rows.time.first(), rows.time.last()) {
            let first_dt = Utc.timestamp_millis_opt(*first).single().unwrap();
            let last_dt = Utc.timestamp_millis_opt(*last).single().unwrap();
            println!(
                "{} bars | funding {} | oi {} | {} -> {}",
                with_state.height(),
                funding.height(),
                oi.height(),
                first_dt,
                last_dt
            );
        } else {
            println!(
                "{} bars | funding {} | oi {}",
                with_state.height(),
                funding.height(),
                oi.height()
            );
        }
        assets.insert(symbol.to_string(), rows);
    }

    let mut aggregate = HashMap::<StateKind, StatLine>::new();
    let mut per_symbol = HashMap::<String, HashMap<StateKind, StatLine>>::new();

    for &symbol in SYMBOLS {
        let rows = assets.get(symbol).unwrap();
        let mut stats = HashMap::<StateKind, StatLine>::new();
        for i in PRICE_LOOKBACK..rows.close.len().saturating_sub(HOLD_BARS) {
            let price_24 = rows.close[i] / rows.close[i - PRICE_LOOKBACK] - 1.0;
            let forward = rows.close[i + HOLD_BARS] / rows.close[i] - 1.0;
            let state = classify_state(
                price_24,
                rows.oi_change_1[i],
                rows.oi_change_7[i],
                rows.oi_z[i],
                rows.funding_z[i],
            );
            stats.entry(state).or_default().add(forward);
            aggregate.entry(state).or_default().add(forward);
        }
        per_symbol.insert(symbol.to_string(), stats);
    }

    println!("\n=== AGGREGATE ACROSS SYMBOLS ===");
    print_table(&aggregate);

    let mut ranking: Vec<(StateKind, f64)> = StateKind::all()
        .iter()
        .map(|s| (*s, aggregate.get(s).map(|x| x.avg_ret_pct()).unwrap_or(0.0)))
        .collect();
    ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    println!("\n=== PER-SYMBOL SNAPSHOT ===");
    for &symbol in SYMBOLS {
        println!("\n--- {} ---", symbol);
        print_table(per_symbol.get(symbol).unwrap());
    }

    println!("\n=== INTERPRETATION ===");
    if let Some((best, best_ret)) = ranking.first() {
        println!(
            "Best aggregate state: {} ({:.2}% avg next-{}h return)",
            best.name(),
            best_ret,
            HOLD_BARS
        );
    }
    if let Some((worst, worst_ret)) = ranking.last() {
        println!(
            "Worst aggregate state: {} ({:.2}% avg next-{}h return)",
            worst.name(),
            worst_ret,
            HOLD_BARS
        );
    }
    println!("- If CrowdedLong and ShortSqueeze are both strong, funding extremes may be momentum / squeeze context rather than fade signals at this horizon.");
    println!("- If HealthyFlush or Capitulation rebounds strongly, the derivatives-state lane may be more useful for tactical de-risking / re-risking than for standalone alpha.");
    println!("- Treat all of this as a short-window state audit only: the public OI depth is still too shallow for confident threshold tuning.");

    Ok(())
}

fn dataframe_to_rows(df: &DataFrame) -> Result<AssetRows> {
    let times = df.column("time")?.cast(&DataType::Int64)?;
    Ok(AssetRows {
        time: times.i64()?.into_iter().map(|v| v.unwrap_or(0)).collect(),
        close: df
            .column("close")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
        funding_z: df
            .column("funding_rate_z")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
        oi_change_1: df
            .column("oi_change_1")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
        oi_change_7: df
            .column("oi_change_7")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
        oi_z: df
            .column("oi_z")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
    })
}

fn classify_state(price_24: f64, oi_1: f64, oi_7: f64, oi_z: f64, funding_z: f64) -> StateKind {
    let price_up = price_24 > 0.03;
    let price_down = price_24 < -0.03;
    let oi_spike = oi_1 > 0.015 || oi_z > 1.2;
    let oi_build = oi_7 > 0.05 || oi_z > 0.8;
    let oi_fade = oi_7 < -0.03 || oi_z < -0.8;
    let funding_hot = funding_z > 1.25;
    let funding_cold = funding_z < -1.25;

    if price_up && oi_build && !funding_hot {
        StateKind::HealthyLong
    } else if price_up && oi_build && funding_hot {
        StateKind::CrowdedLong
    } else if price_up && oi_spike && funding_cold {
        StateKind::ShortSqueeze
    } else if price_down && oi_fade && !funding_cold {
        StateKind::HealthyFlush
    } else if price_down && oi_build && funding_cold {
        StateKind::CrowdedShort
    } else if price_down && oi_spike && funding_hot {
        StateKind::Capitulation
    } else {
        StateKind::Other
    }
}

fn print_table(map: &HashMap<StateKind, StatLine>) {
    println!(
        "{:<18} {:>8} {:>12} {:>10}",
        "State", "Count", "AvgRet", "WinRate"
    );
    for &state in StateKind::all() {
        let row = map.get(&state).cloned().unwrap_or_default();
        println!(
            "{:<18} {:>8} {:>11.2}% {:>9.1}%",
            state.name(),
            row.count,
            row.avg_ret_pct(),
            row.win_rate_pct(),
        );
    }
}
