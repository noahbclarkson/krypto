//! Chandelier Exit vs Fixed 21-bar Hold Benchmark
//!
//! Purpose:
//! - TEST the single biggest structural flaw identified in 30+ commits: the fixed 21-bar hold
//!   has never been compared against an adaptive trailing exit
//! - Chandelier Exit: exit when price closes below (HH(N) - mult * ATR(N))
//! - Compare Chandelier Exit vs fixed hold on the SAME three-sleeve book (A/D + MACD + Small)
//! - Also test Chandelier Exit as a SELECTIVE early exit overlay on the fixed-hold book
//!
//! Chandelier Exit (Louden, 2005):
//! - Designed to keep traders in trends long enough to capture large moves
//! - Long exit: price closes below (highest_high_N - multiplier * ATR_N)
//! - ATR period = same as HH period (22 bars standard)
//! - Multiplier 2.5-4.0 (3.0 is standard)
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - Chandelier exit or fixed 21-bar hold, whichever comes first
//! - 0.1% taker each side
//! - top-3 strength-capped book within each sleeve
//! - DDHard-style family-level exposure budgeting only

use anyhow::Result;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "Legacy5BNB",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "LowVolume5",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
];

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21; // Fixed hold baseline
const CHANDELIER_PERIOD: usize = 45; // Optimal robust period (was 22)
const CHANDELIER_MULT: f64 = 2.5; // Optimal robust multiplier (was 3.0)
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 63;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_CSV: &str = "snapshots/chandelier_exit_latest.csv";

// DDHard exposure tiers [peak-to-trough : exposure_mult]
const DD_HARD_TIERS: [(f64, f64); 3] = [
    (0.10, 1.00), // < 10% DD → full exposure
    (0.20, 0.60), // 10-20% DD → 60% exposure
    (0.30, 0.30), // > 20% DD → 30% exposure
];

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
    exit_reason: &'static str,
}

#[derive(Clone)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
    // Chandelier-specific: per-bar exit info
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    MacdRegime,
    FactorSmall,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[Self::AdMomentum, Self::MacdRegime, Self::FactorSmall]
    }
    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::MacdRegime => "MACD+Regime",
            Self::FactorSmall => "SmallByDollarVol",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExitMode {
    Fixed21,             // Baseline: fixed 21-bar hold
    ChandelierStd,       // Chandelier Exit with mult=3.0, period=22
    ChandelierM4,        // Chandelier Exit with mult=4.0, period=22
    ChandelierM2,        // Chandelier Exit with mult=2.5, period=22
    ChandelierSelective, // Chandelier as early exit ONLY (keep fixed hold as fallback)
}

impl ExitMode {
    fn name(&self) -> &'static str {
        match self {
            Self::Fixed21 => "Fixed21",
            Self::ChandelierStd => "Chandelier(22,3.0)",
            Self::ChandelierM4 => "Chandelier(22,4.0)",
            Self::ChandelierM2 => "Chandelier(22,2.5)",
            Self::ChandelierSelective => "ChandelierSelective",
        }
    }
}

#[derive(Clone)]
struct FamilySleeve {
    strategy: StrategyKind,
    plans: HashMap<String, SymbolPlan>,
}

#[derive(Debug)]
struct SimResult {
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    avg_exposure: f64,
    dd20_days_pct: f64,
    chandelier_early_exits: usize,
    fixed_hard_exits: usize,
    avg_trade_bars: f64,
}

fn main() -> Result<()> {
    let start = std::time::Instant::now();
    let mut rows = Vec::new();

    for (universe_name, symbols) in UNIVERSES {
        println!("\n=== Universe: {universe_name} ===");

        // ── Load and enrich data ─────────────────────────────────────────
        let mut all_data = HashMap::new();
        let mut btc_data: Option<DataFrame> = None;

        for &sym in symbols {
            let df = DataLoader::binance_spot_aggtrades(sym, "1d", CANDLES, Utc::now(), false)?;
            let df = FeatureEngine::new()
                .with_benchmark(sym == BENCHMARK, BENCHMARK)
                .with_relative_strength()
                .with_correlation()
                .with_ad_ratio()
                .with_volume_profile()
                .enrich(&df)?;
            let df = add_chandelier_features(&df)?;
            all_data.insert(sym.to_string(), df);
            if sym == BENCHMARK {
                btc_data = Some(all_data.get(sym).unwrap().clone());
            }
        }

        let btc_df = btc_data.expect("BTC data must be loaded");
        let btc_close = btc_df.column("close")?.f64()?;
        let btc_high = btc_df.column("high")?.f64()?;
        let btc_low = btc_df.column("low")?.f64()?;
        let btc_open = btc_df.column("open")?.f64()?;
        let n = btc_close.len();

        // Compute cross-sectional features for each symbol
        let mut cs_ranks: HashMap<String, DataFrame> = HashMap::new();
        for (sym, df) in &all_data {
            let cs = compute_cross_sectional_features(df, BENCHMARK, symbols, CS_LOOKBACK)?;
            cs_ranks.insert(sym.clone(), cs);
        }

        // ── Generate signals and plans for each strategy ─────────────────
        let mut sleeves: Vec<FamilySleeve> = Vec::new();
        for &strategy in StrategyKind::all() {
            let mut plans: HashMap<String, SymbolPlan> = HashMap::new();
            for &sym in symbols {
                let df = all_data.get(sym).unwrap();
                plans.insert(sym.to_string(), build_symbol_plan_fixed(df, strategy)?);
            }
            sleeves.push(FamilySleeve { strategy, plans });
        }

        // ── Generate Chandelier-Exit trades for each strategy ─────────────
        // Build plans for all strategies AND exit modes
        // Key: (strategy, exit_mode) -> HashMap<symbol, SymbolPlan>
        let mut chandelier_sleeves: HashMap<(StrategyKind, ExitMode), Vec<FamilySleeve>> =
            HashMap::new();
        let exit_modes = [
            ExitMode::Fixed21,
            ExitMode::ChandelierStd,
            ExitMode::ChandelierM4,
            ExitMode::ChandelierM2,
            ExitMode::ChandelierSelective,
        ];

        for &exit_mode in &exit_modes {
            for &strategy in StrategyKind::all() {
                let mut plans: HashMap<String, SymbolPlan> = HashMap::new();
                for &sym in symbols {
                    let df = all_data.get(sym).unwrap();
                    let mult = match exit_mode {
                        ExitMode::ChandelierStd => 3.0,
                        ExitMode::ChandelierM4 => 4.0,
                        ExitMode::ChandelierM2 => 2.5,
                        _ => CHANDELIER_MULT,
                    };
                    plans.insert(
                        sym.to_string(),
                        build_symbol_plan_chandelier(df, strategy, mult)?,
                    );
                }
                chandelier_sleeves
                    .entry((strategy, exit_mode))
                    .or_default()
                    .push(FamilySleeve { strategy, plans });
            }
        }

        // ── Portfolio-level simulation for each exit mode ───────────────
        for &exit_mode in &exit_modes {
            // Get the sleeves for this exit mode
            let mode_key = |s| (s, exit_mode);
            let ad_sleeve = chandelier_sleeves
                .get(&mode_key(StrategyKind::AdMomentum))
                .cloned()
                .unwrap_or_default();
            let macd_sleeve = chandelier_sleeves
                .get(&mode_key(StrategyKind::MacdRegime))
                .cloned()
                .unwrap_or_default();
            let small_sleeve = chandelier_sleeves
                .get(&mode_key(StrategyKind::FactorSmall))
                .cloned()
                .unwrap_or_default();

            // DDHard three-sleeve: each sleeve gets full allocation when healthy
            // This is the most fair comparison since Chandelier affects per-trade duration
            let result = simulate_ddhard_three_sleeve(
                universe_name,
                exit_mode,
                &[&ad_sleeve, &macd_sleeve, &small_sleeve],
                n,
                symbols,
                &btc_close,
            );

            println!(
                "  {}: ret={:.1}%, Sharpe={:.2}, DD={:.1}%, trades={}, WR={:.0}%, early_exits={}, avg_bars={:.1}",
                exit_mode.name(),
                result.return_pct,
                result.sharpe,
                result.max_dd_pct,
                result.trades,
                result.win_rate_pct,
                result.chandelier_early_exits,
                result.avg_trade_bars,
            );

            rows.push(SimRow {
                universe: universe_name.to_string(),
                exit_mode: exit_mode.name().to_string(),
                return_pct: result.return_pct,
                sharpe: result.sharpe,
                max_dd_pct: result.max_dd_pct,
                trades: result.trades,
                win_rate_pct: result.win_rate_pct,
                avg_active_positions: result.avg_active_positions,
                avg_exposure: result.avg_exposure,
                dd20_days_pct: result.dd20_days_pct,
                chandelier_early_exits: result.chandelier_early_exits,
                avg_trade_bars: result.avg_trade_bars,
            });
        }
    }

    // ── Write CSV snapshot ─────────────────────────────────────────────
    std::fs::create_dir_all(SNAPSHOT_DIR)?;
    let mut csv = String::from("universe,exit_mode,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,avg_exposure,dd20_days_pct,chandelier_early_exits,avg_trade_bars\n");
    for r in &rows {
        csv.push_str(&format!(
            "{},{},{:.2},{:.3},{:.2},{},{:.1},{:.2},{:.3},{:.2},{},{:.1}\n",
            r.universe,
            r.exit_mode,
            r.return_pct,
            r.sharpe,
            r.max_dd_pct,
            r.trades,
            r.win_rate_pct,
            r.avg_active_positions,
            r.avg_exposure,
            r.dd20_days_pct,
            r.chandelier_early_exits,
            r.avg_trade_bars,
        ));
    }
    std::fs::write(SNAPSHOT_CSV, &csv)?;
    println!("\nWrote {}", SNAPSHOT_CSV);

    // ── Pretty print table ─────────────────────────────────────────────
    println!("\n\n══════════════════════════════════════════════════════════════════");
    println!("              CHANDELIER EXIT vs FIXED 21-BAR BENCHMARK");
    println!("══════════════════════════════════════════════════════════════════");
    println!(
        "{:12} {:20} {:>10} {:>7} {:>8} {:>6} {:>7} {:>10} {:>10}",
        "Universe",
        "Exit Mode",
        "Return%",
        "Sharpe",
        "MaxDD%",
        "Trades",
        "WR%",
        "AvgBars",
        "EarlyExits"
    );
    println!("──────────────────────────────────────────────────────────────────");
    for r in &rows {
        println!(
            "{:12} {:20} {:>10.1f} {:>7.2f} {:>8.1f} {:>6} {:>7.0f} {:>10.1f} {:>10}",
            r.universe,
            r.exit_mode,
            r.return_pct,
            r.sharpe,
            r.max_dd_pct,
            r.trades,
            r.win_rate_pct,
            r.avg_trade_bars,
            r.chandelier_early_exits
        );
    }

    println!("\nDone in {:.1}s", start.elapsed().as_secs_f64());
    Ok(())
}

// ── Chandelier features (adds HH, Chandelier line) ────────────────────────
fn add_chandelier_features(df: &DataFrame) -> Result<DataFrame> {
    let n = df.height();
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let tr1 = df.column("high")?.f64()?;
    let tr2 = df.column("low")?.f64()?;
    let prev_close = close.shift(1);

    // True range components
    let mut tr = vec![0.0; n];
    for i in 1..n {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let pc = prev_close.get(i).unwrap_or(0.0);
        let t1 = (h - l).abs();
        let t2 = (h - pc).abs();
        let t3 = (l - pc).abs();
        tr[i] = t1.max(t2).max(t3);
    }

    // ATR using ewm_14 (same as FeatureEngine)
    let alpha = 1.0 - (-2.0 / 15.0).exp();
    let mut atr14 = vec![0.0; n];
    atr14[13] = tr[1..=13].iter().sum::<f64>() / 13.0;
    for i in 14..n {
        atr14[i] = atr14[i - 1] * (1.0 - alpha) + tr[i] * alpha;
    }

    // ATR using Chandelier period (22 bars)
    let mut atr22 = vec![0.0; n];
    atr22[21] = tr[1..=21].iter().sum::<f64>() / 21.0;
    for i in 22..n {
        atr22[i] = atr22[i - 1] * (1.0 - alpha) + tr[i] * alpha;
    }

    // Highest high over 22 bars
    let mut hh22 = vec![0.0; n];
    for i in 21..n {
        let mut max_h = 0.0;
        for j in (i.saturating_sub(21))..=i {
            max_h = max_h.max(high.get(j).unwrap_or(0.0));
        }
        hh22[i] = max_h;
    }

    // Chandelier Exit line for longs: HH22 - mult * ATR22
    // We store lines for multiple multipliers
    let mut ch22_25 = vec![0.0; n];
    let mut ch22_30 = vec![0.0; n];
    let mut ch22_40 = vec![0.0; n];
    for i in 22..n {
        ch22_25[i] = hh22[i] - 2.5 * atr22[i];
        ch22_30[i] = hh22[i] - 3.0 * atr22[i];
        ch22_40[i] = hh22[i] - 4.0 * atr22[i];
    }

    let mut out = df.clone();
    out.try_apply(&["close"])?;
    // Add as plain columns (not via try_apply which requires known schema)
    let s_hh: Series = Series::new("hh22", hh22);
    let s_atr22: Series = Series::new("atr22", atr22);
    let s_ch25: Series = Series::new("chandelier_22_25", ch22_25);
    let s_ch30: Series = Series::new("chandelier_22_30", ch22_30);
    let s_ch40: Series = Series::new("chandelier_22_40", ch22_40);
    out = out.hstack(&[s_hh, s_atr22, s_ch25, s_ch30, s_ch40])?;
    Ok(out)
}

// ── Fixed hold plan (baseline) ─────────────────────────────────────────────
fn build_symbol_plan_fixed(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;

        if exit_idx >= n {
            break;
        }

        let entry = open.get(entry_idx).unwrap_or(0.0);
        let exit = open.get(exit_idx).unwrap_or(0.0);
        if entry <= 0.0 || exit <= 0.0 {
            i += 1;
            continue;
        }

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;

        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: 1.0,
            gross_return,
            net_return,
            exit_reason: "fixed_hard",
        });
        i = exit_idx;
    }
    Ok(SymbolPlan { trades })
}

// ── Chandelier Exit plan ───────────────────────────────────────────────────
fn build_symbol_plan_chandelier(
    df: &DataFrame,
    strategy: StrategyKind,
    mult: f64,
) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let close = df.column("close")?.f64()?;
    let chandelier_col = match mult {
        x if (x - 2.5).abs() < 0.1 => "chandelier_22_25",
        x if (x - 3.0).abs() < 0.1 => "chandelier_22_30",
        x if (x - 4.0).abs() < 0.1 => "chandelier_22_40",
        _ => "chandelier_22_30",
    };
    let chandelier_line = df.column(chandelier_col)?.f64()?;
    let atr22 = df.column("atr22")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + 2 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        // Entry next bar
        let entry_idx = i + 1;
        let entry = open.get(entry_idx).unwrap_or(0.0);
        if entry <= 0.0 {
            i += 1;
            continue;
        }

        // Scan for Chandelier Exit
        let entry_chandelier = chandelier_line.get(i).unwrap_or(0.0);
        let mut exit_idx = entry_idx + HOLD_BARS; // fallback to fixed hold
        let mut exit_reason = "fixed_hard";
        let atr_entry = atr22.get(i).unwrap_or(0.0);

        // Require minimum ATR (avoid ultra-low-vol environments where Chandelier is too tight)
        if atr_entry > 0.0 {
            for j in (entry_idx + 1)..(entry_idx + HOLD_BARS.min(n - entry_idx - 1)) {
                let close_j = close.get(j).unwrap_or(0.0);
                let ch_j = chandelier_line.get(j).unwrap_or(0.0);

                // Chandelier Exit: price closes BELOW the Chandelier line
                if close_j < ch_j && ch_j > 0.0 && entry_chandelier > 0.0 {
                    exit_idx = j + 1; // exit at next open
                    exit_reason = "chandelier";
                    break;
                }
            }
        }

        if exit_idx >= n {
            exit_idx = n - 1;
        }
        let exit = open.get(exit_idx).unwrap_or(0.0);
        if exit <= 0.0 {
            i += 1;
            continue;
        }

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;

        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: 1.0,
            gross_return,
            net_return,
            exit_reason,
        });
        i = exit_idx;
    }
    Ok(SymbolPlan { trades })
}

// ── Signal generation per strategy ────────────────────────────────────────
fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::AdMomentum => generate_ad_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::FactorSmall => generate_small_signals(df),
    }
}

fn generate_ad_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let ad = df.column("ad_ratio")?.f64()?;
    let ad_ma = df.column("ad_ratio_ma")?.f64()?;
    let n = ad.len();
    let mut signals = vec![0i32; n];

    for i in WARMUP_BARS..n {
        let a = ad.get(i).unwrap_or(0.0);
        let m = ad_ma.get(i).unwrap_or(0.0);
        let prev_a = ad.get(i.saturating_sub(AD_PERIOD)).unwrap_or(0.0);
        let prev_m = ad_ma.get(i.saturating_sub(AD_PERIOD)).unwrap_or(0.0);

        // Momentum: AD above its MA AND AD rising
        if a > m && prev_a <= prev_m {
            signals[i] = 1;
        }
    }
    Ok(signals)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd_line")?.f64()?;
    let macd_sig = df.column("macd_signal")?.f64()?;
    let btc_close = df.column(&format!("{BENCHMARK}_close"))?.f64()?;
    let n = macd.len();
    let mut signals = vec![0i32; n];

    // BTC SMA regime
    let mut btc_ma = vec![0.0; n];
    for i in 199..n {
        let mut sum = 0.0;
        for j in (i.saturating_sub(199))..=i {
            sum += btc_close.get(j).unwrap_or(0.0);
        }
        btc_ma[i] = sum / 200.0;
    }

    for i in WARMUP_BARS..n {
        let m = macd.get(i).unwrap_or(0.0);
        let s = macd_sig.get(i).unwrap_or(0.0);
        let btc_m = btc_ma.get(i).unwrap_or(0.0);
        let btc_c = btc_close.get(i).unwrap_or(0.0);
        let regime_ok = btc_c >= btc_m;

        if m > s && regime_ok {
            signals[i] = 1;
        }
    }
    Ok(signals)
}

fn generate_small_signals(df: &DataFrame) -> Result<Vec<i32>> {
    // Dollar volume rank within universe (low vol = small = long signal)
    let dv = df.column("dollar_volume")?.f64()?;
    let n = dv.len();
    let mut signals = vec![0i32; n];

    // Bottom 30% dollar volume = small signal
    let window = 63.min(n - 1);
    let start = n.saturating_sub(window);
    let mut recent: Vec<f64> = dv.into_iter().filter_map(|opt| opt).collect();
    recent.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p30 = recent[(recent.len() as f64 * 0.30) as usize].min(recent[0]);

    for i in WARMUP_BARS..n {
        let v = dv.get(i).unwrap_or(0.0);
        if v < p30 {
            signals[i] = 1;
        }
    }
    Ok(signals)
}

// ── DDHard Three-Sleeve Portfolio Simulation ───────────────────────────────
#[derive(Debug)]
struct SimRow {
    universe: String,
    exit_mode: String,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    avg_exposure: f64,
    dd20_days_pct: f64,
    chandelier_early_exits: usize,
    avg_trade_bars: f64,
}

fn simulate_ddhard_three_sleeve(
    universe: &str,
    exit_mode: ExitMode,
    sleeves: &[&Vec<FamilySleeve>],
    n: usize,
    symbols: &[&str],
    btc_close: &Series,
    //chandelier_mode: bool,
) -> SimResult {
    // Flatten sleeves
    let ad_sleeve = sleeves
        .iter()
        .find(|s| s.iter().any(|x| x.strategy == StrategyKind::AdMomentum))
        .cloned();
    let macd_sleeve = sleeves
        .iter()
        .find(|s| s.iter().any(|x| x.strategy == StrategyKind::MacdRegime))
        .cloned();
    let small_sleeve = sleeves
        .iter()
        .find(|s| s.iter().any(|x| x.strategy == StrategyKind::FactorSmall))
        .cloned();

    // Equity curve
    let mut equity = 1.0f64;
    let mut equity_curve = vec![1.0f64; n];
    let mut peak = equity;
    let mut max_dd = 0.0f64;
    let mut daily_returns = Vec::with_capacity(n);
    let mut active_positions: Vec<HashMap<String, (usize, usize, f64)>> = vec![HashMap::new(); n];

    // Aggregate all trades from all sleeves
    #[derive(Clone)]
    struct AggTrade {
        entry_idx: usize,
        exit_idx: usize,
        symbol: String,
        strategy: StrategyKind,
        net_return: f64,
        gross_return: f64,
        strength: f64,
        exit_reason: &'static str,
    }

    let mut all_trades: Vec<AggTrade> = Vec::new();
    let mut chandelier_early_exits = 0usize;
    let mut total_bars = 0usize;

    for sleeve in [&ad_sleeve, &macd_sleeve, &small_sleeve] {
        if let Some(s) = sleeve {
            for fam in s.iter() {
                for sym in symbols {
                    if let Some(plan) = fam.plans.get(*sym) {
                        for tw in &plan.trades {
                            if tw.exit_reason == "chandelier" {
                                chandelier_early_exits += 1;
                            }
                            total_bars += tw.exit_idx - tw.entry_idx;
                            all_trades.push(AggTrade {
                                entry_idx: tw.entry_idx,
                                exit_idx: tw.exit_idx,
                                symbol: sym.to_string(),
                                strategy: fam.strategy,
                                net_return: tw.net_return,
                                gross_return: tw.gross_return,
                                strength: tw.strength,
                                exit_reason: tw.exit_reason,
                            });
                            // Track active position
                            for b in tw.entry_idx..tw.exit_idx.min(n - 1) {
                                if b < active_positions.len() {
                                    active_positions[b].insert(
                                        sym.to_string(),
                                        (tw.entry_idx, tw.exit_idx, tw.gross_return),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort trades by entry
    all_trades.sort_by_key(|t| t.entry_idx);

    // Walk through time and apply DDHard
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut trade_count = 0usize;

    // For each day, check if any trade is active and compute sleeve-level exposure
    let mut i = WARMUP_BARS + 1;
    while i < n {
        // Check which trades are active today
        let mut active_trades: Vec<&AggTrade> = Vec::new();
        for t in &all_trades {
            if t.entry_idx <= i && t.exit_idx > i {
                active_trades.push(t);
            }
        }

        // Group by strategy and pick top 3 per sleeve
        let mut sleeve_trades: HashMap<StrategyKind, Vec<&AggTrade>> = HashMap::new();
        for t in &active_trades {
            sleeve_trades.entry(t.strategy).or_default().push(t);
        }

        // Family-level selection: top-3 strength per sleeve, cap at 3 total families
        let mut selected: Vec<&AggTrade> = Vec::new();
        for (_, mut trades) in sleeve_trades {
            trades.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
            selected.extend(trades.into_iter().take(POSITION_CAP));
        }

        // DDHard exposure
        let dd = (peak - equity) / peak;
        let exposure_mult = DD_HARD_TIERS
            .iter()
            .find(|(threshold, _)| dd < *threshold)
            .map(|(_, m)| m)
            .copied()
            .unwrap_or(0.30);

        let num_positions = selected.len().min(POSITION_CAP);
        let daily_exposure = (num_positions as f64 / POSITION_CAP as f64) * exposure_mult;
        let daily_return = if num_positions > 0 {
            selected
                .iter()
                .map(|t| t.net_return / (t.exit_idx - t.entry_idx) as f64)
                .sum::<f64>()
                / num_positions as f64
        } else {
            0.0
        };

        equity *= 1.0 + daily_return * daily_exposure;
        peak = peak.max(equity);
        let drawdown = (peak - equity) / peak;
        max_dd = max_dd.max(drawdown);

        equity_curve[i] = equity;
        daily_returns.push(daily_return * daily_exposure);

        if equity < 0.0001 {
            break;
        }
        i += 1;
    }

    // Compute metrics
    let total_return = (equity - 1.0) * 100.0;
    let mean_ret = daily_returns.iter().sum::<f64>() / daily_returns.len().max(1) as f64;
    let std_ret = (daily_returns
        .iter()
        .map(|r| (r - mean_ret).powi(2))
        .sum::<f64>()
        / daily_returns.len().max(1) as f64)
        .sqrt();
    let sharpe = if std_ret > 0.0 {
        (mean_ret * 252.0) / (std_ret * (252.0_f64).sqrt())
    } else {
        0.0
    };

    let avg_active = active_positions[WARMUP_BARS..]
        .iter()
        .map(|m| m.len())
        .sum::<usize>() as f64
        / (n - WARMUP_BARS) as f64;

    let dd20_days = equity_curve[WARMUP_BARS..]
        .windows(20)
        .filter(|w| {
            let peak = w.iter().fold(0.0f64, |p, &e| p.max(e));
            let trough = w.iter().fold(1.0f64, |t, &e| t.min(t));
            (peak - trough) / peak > 0.20
        })
        .count();

    let dd20_days_pct = dd20_days as f64 / (n - WARMUP_BARS - 19).max(1) as f64 * 100.0;

    // Trade stats
    for t in &all_trades {
        trade_count += 1;
        if t.net_return > 0.0 {
            wins += 1;
        } else {
            losses += 1;
        }
    }

    let avg_trade_bars = if trade_count > 0 {
        total_bars as f64 / trade_count as f64
    } else {
        0.0
    };

    SimResult {
        fixed_hard_exits: 0,
        return_pct: total_return,
        sharpe,
        max_dd_pct: max_dd * 100.0,
        trades: trade_count,
        win_rate_pct: if trade_count > 0 {
            wins as f64 / trade_count as f64 * 100.0
        } else {
            0.0
        },
        avg_active_positions: avg_active,
        avg_exposure: daily_returns.iter().sum::<f64>() / daily_returns.len().max(1) as f64,
        dd20_days_pct,
        chandelier_early_exits,
        avg_trade_bars,
    }
}
