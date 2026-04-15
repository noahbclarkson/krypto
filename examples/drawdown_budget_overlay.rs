//! Drawdown-budget overlay audit for the current surviving families.
//!
//! Purpose:
//! - Track A trust / portfolio realism first: stop treating drawdown as a side metric
//! - compare simple pre-declared pain-budget overlays on the three current survivors:
//!   A/D Momentum, MACD+Regime, Turtle+MACD
//! - use the same realistic top-3 strength-capped portfolio lens rather than trade-sum headlines
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker each side
//! - top-3 strength-capped book
//!
//! Overlay principle:
//! - no optimization loop; only a few pre-declared overlays
//! - compare drawdown-budget throttles against the earlier 30% USDT high-vol sleeve
//! - exposure is cut as portfolio drawdown deepens or during BTC high-vol, depending on overlay
//! - recovery requires the portfolio to regain some ground, not just one green day

use anyhow::Result;
use chrono::Utc;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const VOL_WINDOW: usize = 21;
const VOL_HIST: usize = 252;
const VOL_PCT_THRESHOLD: f64 = 0.75;
const CP_LOOKBACK: usize = 63;
const CP_Z_THRESHOLD: f64 = 2.25;
const CP_VOL_RATIO_THRESHOLD: f64 = 1.60;
const CP_COOLDOWN_DAYS: usize = 12;

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
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const WARMUP_BARS: usize = 200;
const CS_LOOKBACK: usize = 63;
const TURTLE_PERIOD: usize = 20;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/drawdown_budget_overlay_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/drawdown_budget_overlay_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    MacdRegime,
    TurtleMacd,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[Self::AdMomentum, Self::MacdRegime, Self::TurtleMacd]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleMacd => "Turtle+MACD",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum OverlayKind {
    Baseline,
    DDSoft,
    DDHard,
    DDBreadthRecover,
    DDHardChangePoint,
    DDKillSwitch,
    UsdtVol30,
}

impl OverlayKind {
    fn all() -> &'static [OverlayKind] {
        &[
            Self::Baseline,
            Self::DDSoft,
            Self::DDHard,
            Self::DDBreadthRecover,
            Self::DDHardChangePoint,
            Self::DDKillSwitch,
            Self::UsdtVol30,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Baseline => "Baseline(1.0)",
            Self::DDSoft => "DDSoft(1.0/0.75/0.50)",
            Self::DDHard => "DDHard(1.0/0.60/0.30)",
            Self::DDBreadthRecover => "DDBreadthRecover",
            Self::DDHardChangePoint => "DDHard+ChangePoint",
            Self::DDKillSwitch => "DDKillSwitch(1.0/0.50/0.00)",
            Self::UsdtVol30 => "USDTVol30(1.0/0.70)",
        }
    }

    fn exposure(
        &self,
        dd_pct: f64,
        recovery_ratio: f64,
        btc_vol_pct: f64,
        breadth_ratio: f64,
        cp_guard: f64,
    ) -> f64 {
        match self {
            Self::Baseline => 1.0,
            Self::DDSoft => {
                if dd_pct >= 20.0 && recovery_ratio < 0.95 {
                    0.50
                } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
                    0.75
                } else {
                    1.0
                }
            }
            Self::DDHard => {
                if dd_pct >= 20.0 && recovery_ratio < 0.95 {
                    0.30
                } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
                    0.60
                } else {
                    1.0
                }
            }
            Self::DDBreadthRecover => {
                if dd_pct >= 20.0 {
                    if breadth_ratio < 0.67 {
                        0.15
                    } else if recovery_ratio >= 0.97 {
                        0.45
                    } else {
                        0.35
                    }
                } else if dd_pct >= 10.0 {
                    if breadth_ratio < 0.67 {
                        0.45
                    } else if recovery_ratio >= 0.99 {
                        0.85
                    } else {
                        0.70
                    }
                } else {
                    1.0
                }
            }
            Self::DDHardChangePoint => {
                let dd_hard: f64 = if dd_pct >= 20.0 && recovery_ratio < 0.95 {
                    0.30
                } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
                    0.60
                } else {
                    1.0
                };
                dd_hard.min(cp_guard)
            }
            Self::DDKillSwitch => {
                if dd_pct >= 25.0 && recovery_ratio < 0.95 {
                    0.00
                } else if dd_pct >= 12.0 && recovery_ratio < 0.98 {
                    0.50
                } else {
                    1.0
                }
            }
            Self::UsdtVol30 => {
                if btc_vol_pct >= VOL_PCT_THRESHOLD {
                    0.70
                } else {
                    1.0
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug, Default)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    idle_days_pct: f64,
    avg_exposure: f64,
    dd10_days_pct: f64,
    dd20_days_pct: f64,
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    overlay: OverlayKind,
    stats: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    rows: Vec<ResultRow>,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
    btc_vol_pct: Vec<f64>,
    btc_cp_guard: Vec<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== DRAWDOWN BUDGET OVERLAY AUDIT ===\n");
    println!("Goal: test whether simple pain-budget overlays reduce drawdown honestly before we spend another cycle on signal tweaks");
    println!("New check: add one simple BTC change-point guard and judge it only against DDHard, not against raw baseline");
    println!("Families: A/D Momentum, MACD+Regime, Turtle+MACD");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Portfolio lens: top-{} strength-capped book\n",
        POSITION_CAP
    );

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        data_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        cs_map.insert(symbol.to_string(), data_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        data_cache.insert(symbol, df);
    }

    let mut universe_summaries = Vec::new();
    for &(label, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            label,
            universe_symbols.join(", ")
        );
        let universe = aligned_universe(&data_cache, universe_symbols)?;
        let mut rows = Vec::new();

        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            for &overlay in OverlayKind::all() {
                let stats = simulate_portfolio(
                    &plans,
                    universe.steps,
                    overlay,
                    &universe.btc_vol_pct,
                    &universe.btc_cp_guard,
                );
                rows.push(ResultRow {
                    strategy,
                    overlay,
                    stats,
                });
            }
        }

        rows.sort_by(|a, b| {
            a.stats
                .max_dd_pct
                .partial_cmp(&b.stats.max_dd_pct)
                .unwrap()
                .then_with(|| b.stats.sharpe.partial_cmp(&a.stats.sharpe).unwrap())
                .then_with(|| {
                    b.stats
                        .aligned_return_pct
                        .partial_cmp(&a.stats.aligned_return_pct)
                        .unwrap()
                })
        });

        println!(
            "{:<16} {:<28} {:>10} {:>8} {:>8} {:>7} {:>7} {:>7}",
            "Strategy", "Overlay", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp", ">20DD"
        );
        println!("{}", "-".repeat(106));
        for row in &rows {
            println!(
                "{:<16} {:<28} {:>9.1} {:>8.2} {:>7.1} {:>7} {:>6.2} {:>6.1}%",
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            rows,
        });
    }

    let (archive_md, archive_csv) = write_snapshot(&universe_summaries)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);

    println!("\nInterpretation:");
    println!("- If drawdown budgets help, they should reduce MaxDD and time spent in deep drawdown before we worry about headline return.");
    println!("- If the change-point guard helps, it should improve DDHard by de-risking during BTC transition shocks rather than by living permanently at lower exposure.");
    println!("- If breadth-aware recovery helps, it should beat plain DDHard by re-risking faster only when the capped book has enough live participation.");
    println!("- If only the kill-switch works, the current families remain too path-fragile for full-risk deployment.");
    println!("- This is a sizing / trust audit, not a promotion result.");

    Ok(())
}

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| raw_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let benchmark_df = raw_cache
        .get(BENCHMARK)
        .ok_or_else(|| anyhow::anyhow!("missing benchmark {BENCHMARK}"))?
        .slice(0, min_len);
    let btc_close = benchmark_df.column("close")?.f64()?;
    let mut btc_returns = vec![0.0; benchmark_df.height()];
    for i in 1..benchmark_df.height() {
        let prev = btc_close.get(i - 1).unwrap_or(0.0);
        let now = btc_close.get(i).unwrap_or(0.0);
        if prev > 0.0 {
            btc_returns[i] = (now - prev) / prev;
        }
    }
    let btc_vol_pct = compute_vol_pct(&btc_returns);
    let btc_cp_guard = compute_change_point_guard(&btc_returns);

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }
    Ok(UniverseData {
        data,
        steps,
        btc_vol_pct,
        btc_cp_guard,
    })
}

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| build_symbol_plan(df, symbol, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, symbol: &str, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = signal_for_strategy(df, symbol, strategy)?;
    let strengths = strengths_for_strategy(df, strategy)?;
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
        let entry = match open.get(entry_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };
        let exit = match open.get(exit_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
            net_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio(
    plans: &[SymbolPlan],
    steps: usize,
    overlay: OverlayKind,
    btc_vol_pct: &[f64],
    btc_cp_guard: &[f64],
) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
    let mut dd10_days = 0usize;
    let mut dd20_days = 0usize;
    let mut trade_count = 0usize;
    let mut wins = 0usize;

    for plan in plans {
        for trade in &plan.trades {
            if trade.net_return > 0.0 {
                wins += 1;
            }
            trade_count += 1;
        }
    }

    let mut peak: f64 = 1.0;
    for day in 0..steps {
        let current_equity = equity_curve[day];
        peak = peak.max(current_equity);
        let dd_pct = if peak > 0.0 {
            (1.0 - current_equity / peak) * 100.0
        } else {
            0.0
        };
        let recovery_ratio = if peak > 0.0 {
            current_equity / peak
        } else {
            1.0
        };
        if dd_pct >= 10.0 {
            dd10_days += 1;
        }
        if dd_pct >= 20.0 {
            dd20_days += 1;
        }

        let mut active = Vec::<(f64, f64)>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day == trade.entry_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let span = (trade.exit_idx - trade.entry_idx) as f64;
                    if span > 0.0 {
                        active.push((trade.strength, trade.gross_return / span));
                    }
                }
                if day == trade.exit_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
            }
        }

        active_counts[day] = active.len();
        let breadth_ratio =
            (POSITION_CAP.min(active.len()) as f64 / POSITION_CAP.max(1) as f64).clamp(0.0, 1.0);
        let btc_vol_now = btc_vol_pct.get(day).copied().unwrap_or(0.5);
        let cp_guard = btc_cp_guard.get(day).copied().unwrap_or(1.0);
        let exposure =
            overlay.exposure(dd_pct, recovery_ratio, btc_vol_now, breadth_ratio, cp_guard);
        exposure_sum += exposure;

        if active.is_empty() || exposure <= 0.0 {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = current_equity;
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected_len = POSITION_CAP.min(active.len());
        let selected = &active[..selected_len];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        let scaled_ret = avg_ret * exposure;
        daily_returns[day] = scaled_ret;
        equity_curve[day + 1] = current_equity * (1.0 + scaled_ret);
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    let aligned_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let sharpe = calc_sharpe_from_returns(&daily_returns);
    let max_dd_pct = calc_max_drawdown_pct(&equity_curve);
    let avg_active_positions =
        active_counts.iter().sum::<usize>() as f64 / active_counts.len() as f64;
    let idle_days_pct = active_counts.iter().filter(|&&c| c == 0).count() as f64
        / active_counts.len() as f64
        * 100.0;
    let win_rate_pct = if trade_count == 0 {
        0.0
    } else {
        wins as f64 / trade_count as f64 * 100.0
    };
    let avg_exposure = exposure_sum / steps.max(1) as f64;

    PortfolioStats {
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        trades: trade_count,
        win_rate_pct,
        avg_active_positions,
        idle_days_pct,
        avg_exposure,
        dd10_days_pct: dd10_days as f64 / steps.max(1) as f64 * 100.0,
        dd20_days_pct: dd20_days as f64 / steps.max(1) as f64 * 100.0,
    }
}

fn signal_for_strategy(df: &DataFrame, symbol: &str, strategy: StrategyKind) -> Result<Vec<i32>> {
    Ok(match strategy {
        StrategyKind::AdMomentum => generate_ad_momentum_signals(df, AD_PERIOD)?,
        StrategyKind::MacdRegime => generate_macd_regime_signals(df)?,
        StrategyKind::TurtleMacd => generate_turtle_macd_signals(df, TURTLE_PERIOD)?,
    }
    .into_iter()
    .map(|sig| {
        if strategy == StrategyKind::AdMomentum && symbol.contains("DOGE") {
            sig
        } else {
            sig
        }
    })
    .collect())
}

fn strengths_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::AdMomentum => generate_ad_strengths(df, AD_PERIOD),
        StrategyKind::MacdRegime => generate_macd_strengths(df),
        StrategyKind::TurtleMacd => generate_turtle_strengths(df, TURTLE_PERIOD),
    }
}

fn generate_ad_momentum_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut ad_line = vec![0.0; df.height()];
    for i in 0..df.height() {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let flow = mf * v;
        ad_line[i] = if i == 0 { flow } else { ad_line[i - 1] + flow };
    }

    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let mom = ad_line[i] - ad_line[i - period];
        if mom > 0.0 {
            out[i] = 1;
        } else if mom < 0.0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_ad_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;

    let mut ad_line = vec![0.0; df.height()];
    for i in 0..df.height() {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c = close.get(i).unwrap_or(0.0);
        let v = volume.get(i).unwrap_or(0.0);
        let range = h - l;
        let mf = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let flow = mf * v;
        ad_line[i] = if i == 0 { flow } else { ad_line[i - 1] + flow };
    }

    let mut out = vec![0.0; df.height()];
    for i in period..df.height() {
        out[i] = (ad_line[i] - ad_line[i - period]).abs();
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        if macd_now > macd_sig_now && price > sma_now {
            out[i] = 1;
        } else if macd_now < macd_sig_now && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let macd_dir = if macd_now > macd_sig_now {
            1
        } else if macd_now < macd_sig_now {
            -1
        } else {
            0
        };
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let turtle = if price > highest {
            1
        } else if price < lowest {
            -1
        } else {
            0
        };
        if turtle != 0 && turtle == macd_dir {
            out[i] = turtle;
        }
    }
    Ok(out)
}

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    Ok((0..macd.len())
        .map(|i| (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs())
        .collect())
}

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let range = (period_high - period_low).abs().max(1e-9);
        if current_close > period_high {
            out[i] = (current_close - period_high) / range;
        } else if current_close < period_low {
            out[i] = (period_low - current_close) / range;
        }
    }
    Ok(out)
}

fn compute_vol_pct(returns: &[f64]) -> Vec<f64> {
    let n = returns.len();
    let mut pct = vec![0.5; n];
    for i in VOL_HIST..n {
        let mut cur_sq = 0.0;
        for j in i.saturating_sub(VOL_WINDOW)..i {
            cur_sq += returns[j] * returns[j];
        }
        let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

        let mut hist: Vec<f64> = Vec::with_capacity(VOL_HIST);
        for w in i.saturating_sub(VOL_HIST)..i.saturating_sub(VOL_WINDOW) {
            let mut sq = 0.0;
            for j in w..(w + VOL_WINDOW) {
                if j < returns.len() {
                    sq += returns[j] * returns[j];
                }
            }
            hist.push((sq / VOL_WINDOW as f64).sqrt());
        }

        if !hist.is_empty() {
            let count_below = hist.iter().filter(|&&v| v <= cur_vol).count();
            pct[i] = count_below as f64 / hist.len() as f64;
        }
    }
    pct
}

fn compute_change_point_guard(returns: &[f64]) -> Vec<f64> {
    let n = returns.len();
    let mut guard = vec![1.0; n];
    let mut cooldown = 0usize;

    for i in 0..n {
        if cooldown > 0 {
            guard[i] = 0.45;
            cooldown -= 1;
        }

        if i < CP_LOOKBACK || i < VOL_WINDOW {
            continue;
        }

        let hist = &returns[i - CP_LOOKBACK..i];
        let mean = hist.iter().sum::<f64>() / hist.len() as f64;
        let var = hist
            .iter()
            .map(|r| {
                let d = *r - mean;
                d * d
            })
            .sum::<f64>()
            / hist.len() as f64;
        let std = var.sqrt();
        if std <= 1e-9 {
            continue;
        }

        let z = ((returns[i] - mean) / std).abs();

        let cur_vol = {
            let slice = &returns[i - VOL_WINDOW + 1..=i];
            (slice.iter().map(|r| r * r).sum::<f64>() / slice.len() as f64).sqrt()
        };

        let mut hist_vol = Vec::new();
        for end in CP_LOOKBACK..i {
            if end + 1 < VOL_WINDOW {
                continue;
            }
            let start = end + 1 - VOL_WINDOW;
            let slice = &returns[start..=end];
            hist_vol.push((slice.iter().map(|r| r * r).sum::<f64>() / slice.len() as f64).sqrt());
        }
        hist_vol.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_vol = if hist_vol.is_empty() {
            0.0
        } else {
            hist_vol[hist_vol.len() / 2]
        };
        let vol_ratio = if median_vol > 1e-9 {
            cur_vol / median_vol
        } else {
            1.0
        };

        if z >= CP_Z_THRESHOLD || vol_ratio >= CP_VOL_RATIO_THRESHOLD {
            cooldown = CP_COOLDOWN_DAYS;
            guard[i] = 0.45;
        }
    }

    guard
}

fn calculate_sma(values: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= values.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std = var.sqrt();
    if std <= 1e-12 {
        0.0
    } else {
        mean / std * 365.0f64.sqrt()
    }
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        let dd = (value / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    max_dd.abs()
}

fn write_snapshot(universes: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/drawdown_budget_overlay_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/drawdown_budget_overlay_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# Drawdown Budget Overlay Audit\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Top-{} strength-capped book\n", POSITION_CAP));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n\n", TAKER_FEE * 100.0));

    for universe in universes {
        markdown.push_str(&format!("## {}\n\n", universe.label));
        markdown.push_str("| Rank | Strategy | Overlay | Return % | Sharpe | MaxDD % | Trades | Win % | Avg Exposure | DD>=10 days % | DD>=20 days % |\n");
        markdown.push_str("|------|----------|---------|---------:|-------:|--------:|-------:|------:|-------------:|--------------:|--------------:|\n");
        for (idx, row) in universe.rows.iter().enumerate() {
            markdown.push_str(&format!(
                "| {} | {} | {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.1} |\n",
                idx + 1,
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_exposure,
                row.stats.dd10_days_pct,
                row.stats.dd20_days_pct,
            ));
        }
        markdown.push('\n');
    }

    let mut csv = String::from("universe,rank,strategy,overlay,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,dd10_days_pct,dd20_days_pct\n");
    for universe in universes {
        for (idx, row) in universe.rows.iter().enumerate() {
            csv.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                universe.label,
                idx + 1,
                row.strategy.name(),
                row.overlay.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.dd10_days_pct,
                row.stats.dd20_days_pct,
            ));
        }
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}
