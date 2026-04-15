//! A/D symbol-aware allocation stress test.
//!
//! Goal:
//! - keep the same harsh-universe / chronology-first lens used for the recent A/D breadth result
//! - test whether fixed symbol-aware sizing (ADA overweight, DOGE underweight) improves the raw
//!   A/D family without pretending a local parameter search is a new edge
//!
//! Important discipline:
//! - fixed, pre-declared tilts only
//! - same next-open / fixed-hold / taker-fee assumptions as the existing harsh harness
//! - this is a trust / attribution follow-up, not a promotion decision

use anyhow::{Context, Result};
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::{BTreeSet, HashMap};

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 54; // Optimized: was 21, winner from full 3-63 integer sweep (composite score 0.2855, WF Sharpe +0.49)
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES_PER_WINDOW: usize = 30;
const RESAMPLE_BLOCKS: usize = 6;
const RESAMPLE_TRAIN_BLOCKS: usize = 4;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47) // hyperopt 2026-04-06

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    AdMomentumNoDOGE,
    AdAdaOverweightDogeUnderweight,
    AdAdaDoubleNoDOGE,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::AdMomentum,
            Self::AdMomentumNoDOGE,
            Self::AdAdaOverweightDogeUnderweight,
            Self::AdAdaDoubleNoDOGE,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::AdMomentumNoDOGE => "A/D Momentum (NoDOGE)",
            Self::AdAdaOverweightDogeUnderweight => "A/D ADA1.5x DOGE0.5x",
            Self::AdAdaDoubleNoDOGE => "A/D ADA2x NoDOGE",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::AdMomentum => "raw A/D breadth baseline",
            Self::AdMomentumNoDOGE => "DOGE fully removed",
            Self::AdAdaOverweightDogeUnderweight => "light symbol-aware tilt from decomposition",
            Self::AdAdaDoubleNoDOGE => "strong ADA tilt + DOGE removal",
        }
    }
}

#[derive(Default, Clone, Debug)]
struct StrategyResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
}

impl StrategyResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }
}

#[derive(Clone, Debug)]
struct SummaryRow {
    kind: StrategyKind,
    full_return_pct: f64,
    full_trades: usize,
    full_win_rate: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== A/D SYMBOL-AWARE STRESS ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Symbol-aware hypothesis: ADA deserves more size; DOGE deserves less.\n");

    let loader = DataLoader::new(None, None);
    let mut data_cache = HashMap::<String, DataFrame>::new();

    for &symbol in LOAD_SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", enriched.height());
        data_cache.insert(symbol.to_string(), enriched);
    }

    let mut win_counts = HashMap::<StrategyKind, usize>::new();

    for &(universe_name, universe_symbols) in UNIVERSES {
        println!(
            "\n--- Universe: {} ({}) ---",
            universe_name,
            universe_symbols.join(", ")
        );
        let quarter_windows = quarter_windows(&data_cache, universe_symbols)?;
        let resample_sets = cpcv_style_windows(&data_cache, universe_symbols)?;

        let mut rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let full = evaluate_strategy(
                &data_cache,
                universe_symbols,
                strategy,
                &[(
                    0,
                    min_symbol_len(&data_cache, universe_symbols)?.saturating_sub(HOLD_BARS + 1),
                )],
            )?;

            let mut wf_passed = 0usize;
            for &(start, end) in &quarter_windows {
                let eval =
                    evaluate_strategy(&data_cache, universe_symbols, strategy, &[(start, end)])?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut resample_passed = 0usize;
            for windows in &resample_sets {
                let eval = evaluate_strategy(&data_cache, universe_symbols, strategy, windows)?;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    resample_passed += 1;
                }
            }

            rows.push(SummaryRow {
                kind: strategy,
                full_return_pct: full.total_return_pct,
                full_trades: full.trades,
                full_win_rate: full.win_rate(),
                wf_passed,
                wf_total: quarter_windows.len(),
                resample_passed,
                resample_total: resample_sets.len(),
            });
        }

        rows.sort_by(|a, b| {
            b.resample_passed
                .cmp(&a.resample_passed)
                .then_with(|| b.wf_passed.cmp(&a.wf_passed))
                .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
        });

        for row in &rows {
            println!(
                "{:<24} {:>10.1} {:>7} {:>7.1}% {:>4}/{} {:>6}/{}  {}",
                row.kind.name(),
                row.full_return_pct,
                row.full_trades,
                row.full_win_rate * 100.0,
                row.wf_passed,
                row.wf_total,
                row.resample_passed,
                row.resample_total,
                row.kind.description(),
            );
        }

        if let Some(best) = rows.first() {
            *win_counts.entry(best.kind).or_insert(0) += 1;
        }
    }

    println!("\n=== UNIVERSE WIN COUNTS ===");
    for &strategy in StrategyKind::all() {
        println!(
            "{:<24} {:>2}/{}",
            strategy.name(),
            win_counts.get(&strategy).copied().unwrap_or(0),
            UNIVERSES.len()
        );
    }

    println!("\nInterpretation:");
    println!("- A symbol tilt only earns attention if it improves chronology or hostile-basket breadth without becoming a DOGE-delete story in disguise.");
    println!("- A/D already survived the hostile baskets; this test asks whether the per-symbol decomposition is actionable, not whether A/D needs more headline inflation.");
    println!("- This is still a breadth/trust follow-up, not Hall-of-Fame evidence.");

    Ok(())
}

fn evaluate_strategy(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: StrategyKind,
    windows: &[(usize, usize)],
) -> Result<StrategyResult> {
    let mut out = StrategyResult::default();
    for &symbol in symbols {
        let df = data_cache.get(symbol).context("missing symbol data")?;
        let signal = signal_for_strategy(df, strategy, symbol)?;
        let weight = symbol_weight(strategy, symbol);
        let result = backtest_fixed_hold_next_open_window(df, &signal, windows, weight)?;
        out.total_return_pct += result.total_return_pct;
        out.trades += result.trades;
        out.wins += result.wins;
    }
    Ok(out)
}

fn signal_for_strategy(df: &DataFrame, strategy: StrategyKind, symbol: &str) -> Result<Vec<i32>> {
    let ad_momentum = generate_ad_momentum_signals(df, AD_PERIOD)?;
    let out = match strategy {
        StrategyKind::AdMomentum => ad_momentum,
        StrategyKind::AdMomentumNoDOGE | StrategyKind::AdAdaDoubleNoDOGE => {
            if symbol.contains("DOGE") {
                vec![0; df.height()]
            } else {
                ad_momentum
            }
        }
        StrategyKind::AdAdaOverweightDogeUnderweight => ad_momentum,
    };
    Ok(out)
}

fn symbol_weight(strategy: StrategyKind, symbol: &str) -> f64 {
    match strategy {
        StrategyKind::AdMomentum | StrategyKind::AdMomentumNoDOGE => 1.0,
        StrategyKind::AdAdaOverweightDogeUnderweight => {
            if symbol.contains("ADA") {
                1.5
            } else if symbol.contains("DOGE") {
                0.5
            } else {
                1.0
            }
        }
        StrategyKind::AdAdaDoubleNoDOGE => {
            if symbol.contains("DOGE") {
                0.0
            } else if symbol.contains("ADA") {
                2.0
            } else {
                1.0
            }
        }
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

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    windows: &[(usize, usize)],
    weight: f64,
) -> Result<StrategyResult> {
    let open = df.column("open")?.f64()?;
    let mut out = StrategyResult::default();

    if weight <= 0.0 {
        return Ok(out);
    }

    for &(start, end) in windows {
        let mut i = start.max(1);
        while i + HOLD_BARS < end && i + HOLD_BARS < df.height() {
            let signal = signals[i - 1];
            if signal == 0 {
                i += 1;
                continue;
            }
            let entry = open.get(i).unwrap_or(0.0);
            let exit = open.get(i + HOLD_BARS).unwrap_or(0.0);
            if entry <= 0.0 || exit <= 0.0 {
                i += 1;
                continue;
            }

            let gross = if signal > 0 {
                (exit / entry) - 1.0
            } else {
                (entry / exit) - 1.0
            };
            let net = (gross - (2.0 * TAKER_FEE)) * weight;
            out.total_return_pct += net * 100.0;
            out.trades += 1;
            if net > 0.0 {
                out.wins += 1;
            }
            i += HOLD_BARS;
        }
    }

    Ok(out)
}

fn quarter_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let len = min_symbol_len(data_cache, symbols)?;
    let usable = len.saturating_sub(HOLD_BARS + 1);
    let quarter = usable / 4;
    let mut out = Vec::new();
    for k in 0..4 {
        let start = k * quarter;
        let end = if k == 3 { usable } else { (k + 1) * quarter };
        out.push((start, end));
    }
    Ok(out)
}

fn cpcv_style_windows(
    data_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let len = min_symbol_len(data_cache, symbols)?;
    let usable = len.saturating_sub(HOLD_BARS + 1);
    let block = usable / RESAMPLE_BLOCKS;
    let ranges: Vec<(usize, usize)> = (0..RESAMPLE_BLOCKS)
        .map(|i| {
            let start = i * block;
            let end = if i == RESAMPLE_BLOCKS - 1 {
                usable
            } else {
                (i + 1) * block
            };
            (start, end)
        })
        .collect();

    let mut out = Vec::new();
    for test_blocks in combinations(RESAMPLE_BLOCKS, RESAMPLE_BLOCKS - RESAMPLE_TRAIN_BLOCKS) {
        let test_set: BTreeSet<usize> = test_blocks.into_iter().collect();
        let windows = ranges
            .iter()
            .enumerate()
            .filter_map(|(idx, range)| {
                if test_set.contains(&idx) {
                    Some(*range)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        out.push(windows);
    }
    Ok(out)
}

fn min_symbol_len(data_cache: &HashMap<String, DataFrame>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .map(|symbol| {
            data_cache
                .get(*symbol)
                .map(|df| df.height())
                .with_context(|| format!("missing df for {}", symbol))
        })
        .collect::<Result<Vec<_>>>()
        .map(|lens| lens.into_iter().min().unwrap_or(0))
}

fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }

    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}
