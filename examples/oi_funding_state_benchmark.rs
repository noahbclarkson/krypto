//! OI + funding state benchmark.
//!
//! Purpose:
//! - build the missing open-interest data layer honestly
//! - test whether simple derivatives crowding states have directional value
//!   before pasting them onto any benchmark leader as a filter
//!
//! State logic (evaluated on next 21-bar return, using signal at close):
//! - Healthy participation: price 7d up + OI 7d up + funding not extreme
//! - Crowded long:         price 7d up + OI 7d up + funding extremely positive
//! - Weak rally:           price 7d up + OI flat/down
//! - Crowded short:        price 7d down + OI 7d up + funding extremely negative
//! - Healthy flush:        price 7d down + OI down + funding negative but not extreme
//!
//! If the state layer is useful, healthy participation should have better forward
//! returns than crowded/weak states without requiring another local tuning loop.

use anyhow::Result;
use krypto::{
    data::{
        align_funding_to_ohlcv, align_open_interest_to_ohlcv, DataLoader, FundingRateLoader,
        OpenInterestLoader,
    },
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::cmp::Ordering;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const OI_WINDOW: usize = 30;
const FUNDING_Z_WINDOW: usize = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StateKind {
    HealthyParticipation,
    CrowdedLong,
    WeakRally,
    CrowdedShort,
    HealthyFlush,
    Other,
}

impl StateKind {
    fn all() -> &'static [StateKind] {
        &[
            Self::HealthyParticipation,
            Self::CrowdedLong,
            Self::WeakRally,
            Self::CrowdedShort,
            Self::HealthyFlush,
            Self::Other,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::HealthyParticipation => "HealthyParticipation",
            Self::CrowdedLong => "CrowdedLong",
            Self::WeakRally => "WeakRally",
            Self::CrowdedShort => "CrowdedShort",
            Self::HealthyFlush => "HealthyFlush",
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
    close: Vec<f64>,
    oi_change_7: Vec<f64>,
    funding_z: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== OI + FUNDING STATE BENCHMARK ===\n");
    println!("Lens: classify derivatives crowding states at close, score next {}-bar close-to-close return", HOLD_BARS);
    println!("Goal: build state evidence before any strategy-overlay claim\n");

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");
    let oi_loader = OpenInterestLoader::with_cache_dir("examples/open_interest_cache");

    let mut assets = HashMap::<String, AssetRows>::new();

    for &symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let tech = FeatureEngine::add_technicals(&raw, None)?;
        let funding = funding_loader.fetch(symbol, None, None).await?;
        let oi = oi_loader.fetch(symbol, None, None).await?;
        let with_funding = align_funding_to_ohlcv(&tech, &funding, FUNDING_Z_WINDOW)?;
        let with_state = align_open_interest_to_ohlcv(&with_funding, &oi, OI_WINDOW)?;
        println!(
            "{} bars | funding {} | oi {}",
            with_state.height(),
            funding.height(),
            oi.height()
        );
        assets.insert(symbol.to_string(), dataframe_to_rows(&with_state)?);
    }

    let mut aggregate = HashMap::<StateKind, StatLine>::new();
    let mut per_symbol = HashMap::<String, HashMap<StateKind, StatLine>>::new();

    for &symbol in SYMBOLS {
        let rows = assets.get(symbol).unwrap();
        let mut stats = HashMap::<StateKind, StatLine>::new();
        for i in 7..rows.close.len().saturating_sub(HOLD_BARS) {
            let price_7 = rows.close[i] / rows.close[i - 7] - 1.0;
            let forward = rows.close[i + HOLD_BARS] / rows.close[i] - 1.0;
            let state = classify_state(price_7, rows.oi_change_7[i], rows.funding_z[i]);
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
            "Best aggregate state: {} ({:.2}% avg next-{}-bar return)",
            best.name(),
            best_ret,
            HOLD_BARS
        );
    }
    if let Some((worst, worst_ret)) = ranking.last() {
        println!(
            "Worst aggregate state: {} ({:.2}% avg next-{}-bar return)",
            worst.name(),
            worst_ret,
            HOLD_BARS
        );
    }
    println!("- If HealthyParticipation beats CrowdedLong and WeakRally, the state layer is promising as confirmation / de-risking rather than standalone alpha.");
    println!("- If CrowdedLong is still strong, then extreme funding is not yet a reliable fade condition in this simple daily lens.");
    println!("- This is state evidence only: no claim yet that any benchmark family improves after filtering on it.");

    Ok(())
}

fn dataframe_to_rows(df: &DataFrame) -> Result<AssetRows> {
    Ok(AssetRows {
        close: df
            .column("close")?
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
        funding_z: df
            .column("funding_rate_z")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect(),
    })
}

fn classify_state(price_7: f64, oi_7: f64, funding_z: f64) -> StateKind {
    let funding_extreme_pos = funding_z > 1.5;
    let funding_extreme_neg = funding_z < -1.5;
    let oi_up = oi_7 > 0.03;
    let oi_down = oi_7 < -0.01;
    let price_up = price_7 > 0.05;
    let price_down = price_7 < -0.05;

    if price_up && oi_up && !funding_extreme_pos {
        StateKind::HealthyParticipation
    } else if price_up && oi_up && funding_extreme_pos {
        StateKind::CrowdedLong
    } else if price_up && !oi_up {
        StateKind::WeakRally
    } else if price_down && oi_up && funding_extreme_neg {
        StateKind::CrowdedShort
    } else if price_down && oi_down && !funding_extreme_neg {
        StateKind::HealthyFlush
    } else {
        StateKind::Other
    }
}

fn print_table(map: &HashMap<StateKind, StatLine>) {
    println!(
        "{:<22} {:>8} {:>12} {:>10}",
        "State", "Count", "AvgRet", "WinRate"
    );
    for &state in StateKind::all() {
        let row = map.get(&state).cloned().unwrap_or_default();
        println!(
            "{:<22} {:>8} {:>11.2}% {:>9.1}%",
            state.name(),
            row.count,
            row.avg_ret_pct(),
            row.win_rate_pct(),
        );
    }
}
