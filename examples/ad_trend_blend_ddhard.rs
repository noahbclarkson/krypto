//! A/D + trend-representative blend test under the realistic DDHard allocator lens.
//!
//! Purpose:
//! - answer the current highest-value structural question from PLAN.md directly:
//!   does blending the least-overlapping survivor (A/D) with one trend representative
//!   produce a more credible capped-book process than either row alone?
//! - keep the allocator fixed at the current baseline (`DDHard`) and avoid local weight tuning
//! - use a conservative per-symbol blend rule:
//!   - if only one family is active -> use it
//!   - if both agree -> keep the shared direction
//!   - if they conflict -> go flat
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker each side
//! - top-3 strength-capped book
//! - DDHard exposure overlay only

use anyhow::Result;
use chrono::Utc;
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

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
const SNAPSHOT_LATEST_MD: &str = "snapshots/ad_trend_blend_ddhard_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/ad_trend_blend_ddhard_latest.csv";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    AdMomentum,
    CTRend,
    MacdRegime,
    BlendAdCTRend,
    BlendAdMacdRegime,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::AdMomentum,
            Self::CTRend,
            Self::MacdRegime,
            Self::BlendAdCTRend,
            Self::BlendAdMacdRegime,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::AdMomentum => "A/D Momentum",
            Self::CTRend => "CTREND",
            Self::MacdRegime => "MACD+Regime",
            Self::BlendAdCTRend => "Blend(A/D+CTREND)",
            Self::BlendAdMacdRegime => "Blend(A/D+MACD+Regime)",
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
    dd20_days_pct: f64,
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== A/D + TREND BLEND UNDER DDHARD ===\n");
    println!("Question: does the least-overlapping survivor (A/D) improve a trend representative under the current realistic allocator baseline?");
    println!("Blend rule: keep one family when the other is idle, keep agreement when both align, go flat on conflicts.");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Portfolio lens: top-{} strength-capped book with DDHard overlay\n",
        POSITION_CAP
    );

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut data_cache = HashMap::<String, DataFrame>::new();
    data_cache.insert(BENCHMARK.to_string(), bench_df.clone());
    for &symbol in LOAD_SYMBOLS.iter().filter(|&&s| s != BENCHMARK) {
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
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
            let stats = simulate_portfolio_ddhard(&plans, universe.steps);
            rows.push(ResultRow { strategy, stats });
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
            "{:<24} {:>10} {:>8} {:>8} {:>7} {:>7} {:>7}",
            "Strategy", "Ret%", "Sharpe", "MaxDD", "Trades", "Exp", ">20DD"
        );
        println!("{}", "-".repeat(82));
        for row in &rows {
            println!(
                "{:<24} {:>9.1} {:>8.2} {:>7.1} {:>7} {:>6.2} {:>6.1}%",
                row.strategy.name(),
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
    println!("- A blend only earns attention if it improves drawdown shape or breadth credibly, not just one raw-return headline.");
    println!(
        "- DDHard is fixed here on purpose: this is a structure test, not another allocator loop."
    );
    println!("- If the blends only repackage the same basin, standalone rows should stay cleaner.");
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

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }
    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(symbol, df)| build_symbol_plan(df, symbol, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, _symbol: &str, strategy: StrategyKind) -> Result<SymbolPlan> {
    let (signals, strengths) = signal_and_strength_for_strategy(df, strategy)?;
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

fn simulate_portfolio_ddhard(plans: &[SymbolPlan], steps: usize) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut exposure_sum = 0.0;
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
        let exposure = ddhard_exposure(dd_pct, recovery_ratio);
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
        dd20_days_pct: dd20_days as f64 / steps.max(1) as f64 * 100.0,
    }
}

fn ddhard_exposure(dd_pct: f64, recovery_ratio: f64) -> f64 {
    if dd_pct >= 20.0 && recovery_ratio < 0.95 {
        0.30
    } else if dd_pct >= 10.0 && recovery_ratio < 0.98 {
        0.60
    } else {
        1.0
    }
}

fn signal_and_strength_for_strategy(
    df: &DataFrame,
    strategy: StrategyKind,
) -> Result<(Vec<i32>, Vec<f64>)> {
    match strategy {
        StrategyKind::AdMomentum => Ok((
            generate_ad_momentum_signals(df, AD_PERIOD)?,
            generate_ad_strengths(df, AD_PERIOD)?,
        )),
        StrategyKind::CTRend => Ok(generate_ctrend_signal_strength(df)?),
        StrategyKind::MacdRegime => Ok((
            generate_macd_regime_signals(df)?,
            generate_macd_strengths(df)?,
        )),
        StrategyKind::BlendAdCTRend => {
            let ad_sig = generate_ad_momentum_signals(df, AD_PERIOD)?;
            let ad_str = generate_ad_strengths(df, AD_PERIOD)?;
            let (tr_sig, tr_str) = generate_ctrend_signal_strength(df)?;
            Ok(blend_signals_and_strengths(
                &ad_sig, &ad_str, &tr_sig, &tr_str,
            ))
        }
        StrategyKind::BlendAdMacdRegime => {
            let ad_sig = generate_ad_momentum_signals(df, AD_PERIOD)?;
            let ad_str = generate_ad_strengths(df, AD_PERIOD)?;
            let tr_sig = generate_macd_regime_signals(df)?;
            let tr_str = generate_macd_strengths(df)?;
            Ok(blend_signals_and_strengths(
                &ad_sig, &ad_str, &tr_sig, &tr_str,
            ))
        }
    }
}

fn blend_signals_and_strengths(
    sig_a: &[i32],
    str_a: &[f64],
    sig_b: &[i32],
    str_b: &[f64],
) -> (Vec<i32>, Vec<f64>) {
    let len = sig_a
        .len()
        .min(sig_b.len())
        .min(str_a.len())
        .min(str_b.len());
    let mut out_sig = vec![0; len];
    let mut out_str = vec![0.0; len];
    for i in 0..len {
        let a = sig_a[i];
        let b = sig_b[i];
        out_sig[i] = if a == 0 && b == 0 {
            0
        } else if a == 0 {
            b
        } else if b == 0 {
            a
        } else if a == b {
            a
        } else {
            0
        };
        out_str[i] = match out_sig[i] {
            0 => 0.0,
            s if s == a && s == b => 0.5 * (str_a[i].abs() + str_b[i].abs()),
            s if s == a => str_a[i].abs(),
            _ => str_b[i].abs(),
        };
    }
    (out_sig, out_str)
}

fn generate_ctrend_signal_strength(df: &DataFrame) -> Result<(Vec<i32>, Vec<f64>)> {
    let close = df.column("close")?.f64()?;
    let volume = df.column("volume")?.f64()?;
    let n = df.height();

    let vol_sma_20 = calculate_sma(&volume, 20);
    let vol_sma_63 = calculate_sma(&volume, 63);
    let ret_5 = rolling_return(&close, 5);
    let ret_21 = rolling_return(&close, 21);
    let ret_63 = rolling_return(&close, 63);
    let ret_126 = rolling_return(&close, 126);
    let rv_21 = rolling_realized_vol(&close, 21);
    let rv_63 = rolling_realized_vol(&close, 63);

    let mut out_sig = vec![0i32; n];
    let mut out_str = vec![0.0; n];
    for i in 126..n {
        let short_vol = rv_21[i].max(1e-6);
        let med_vol = rv_63[i].max(1e-6);
        let price_score = 0.15 * (ret_5[i] / short_vol)
            + 0.35 * (ret_21[i] / short_vol)
            + 0.30 * (ret_63[i] / med_vol)
            + 0.20 * (ret_126[i] / med_vol);

        let vol_ratio_fast = if vol_sma_20[i] > 1e-9 {
            volume.get(i).unwrap_or(0.0) / vol_sma_20[i]
        } else {
            1.0
        };
        let vol_ratio_slow = if vol_sma_63[i] > 1e-9 {
            vol_sma_20[i] / vol_sma_63[i]
        } else {
            1.0
        };
        let price_dir = if ret_21[i] > 0.0 {
            1.0
        } else if ret_21[i] < 0.0 {
            -1.0
        } else {
            0.0
        };
        let short_dir = if ret_5[i] > 0.0 {
            1.0
        } else if ret_5[i] < 0.0 {
            -1.0
        } else {
            0.0
        };
        let volume_score = 0.20 * (vol_ratio_fast.ln()).clamp(-1.5, 1.5) * short_dir
            + 0.20 * (vol_ratio_slow.ln()).clamp(-1.5, 1.5) * price_dir;

        let score = price_score + volume_score;
        out_str[i] = score.abs();
        if score > 0.35 {
            out_sig[i] = 1;
        } else if score < -0.35 {
            out_sig[i] = -1;
        }
    }
    Ok((out_sig, out_str))
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

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    Ok((0..macd.len())
        .map(|i| (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs())
        .collect())
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = *r - mean;
            d * d
        })
        .sum::<f64>()
        / (n as f64 - 1.0);
    if var <= 1e-12 {
        return 0.0;
    }
    mean / var.sqrt() * (252.0_f64).sqrt()
}

fn calc_max_drawdown_pct(equity: &[f64]) -> f64 {
    let mut peak = equity.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &v in equity {
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = 1.0 - v / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd * 100.0
}

fn rolling_return(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - lookback).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            out[i] = (now / prev).ln();
        }
    }
    out
}

fn rolling_realized_vol(values: &Float64Chunked, lookback: usize) -> Vec<f64> {
    let mut rets = vec![0.0; values.len()];
    for i in 1..values.len() {
        let now = values.get(i).unwrap_or(0.0);
        let prev = values.get(i - 1).unwrap_or(0.0);
        if now > 0.0 && prev > 0.0 {
            rets[i] = (now / prev).ln();
        }
    }
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        let window = &rets[i - lookback + 1..=i];
        let n = window.len() as f64;
        let mean = window.iter().sum::<f64>() / n;
        let var = window.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;
        out[i] = var.max(0.0).sqrt();
    }
    out
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

fn write_snapshot(universe_summaries: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/ad_trend_blend_ddhard_{}.md", SNAPSHOT_DIR, ts);
    let archive_csv = format!("{}/ad_trend_blend_ddhard_{}.csv", SNAPSHOT_DIR, ts);

    let mut md = String::from("# A/D + Trend Blend under DDHard\n\n");
    let mut csv = String::from("universe,strategy,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_active_positions,idle_days_pct,avg_exposure,dd20_days_pct\n");

    for universe in universe_summaries {
        md.push_str(&format!("## {}\n\n", universe.label));
        md.push_str("| Strategy | Return % | Sharpe | MaxDD % | Trades | Win Rate % | Avg Active | Idle % | Avg Exp | >20DD % |\n");
        md.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
        for row in &universe.rows {
            md.push_str(&format!(
                "| {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} | {:.1} | {:.2} | {:.1} |\n",
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
            ));
            csv.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                universe.label,
                row.strategy.name(),
                row.stats.aligned_return_pct,
                row.stats.sharpe,
                row.stats.max_dd_pct,
                row.stats.trades,
                row.stats.win_rate_pct,
                row.stats.avg_active_positions,
                row.stats.idle_days_pct,
                row.stats.avg_exposure,
                row.stats.dd20_days_pct,
            ));
        }
        md.push('\n');
    }

    fs::write(SNAPSHOT_LATEST_MD, &md)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}
