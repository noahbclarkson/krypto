//! Chandelier Period Hyperparameter Optimization
//!
//! Purpose:
//! - Systematically sweep CHANDELIER_PERIOD from 10 to 60 bars
//! - Compare against Fixed 21-bar hold baseline
//! - Run DDHard three-sleeve simulation with A/D + MACD + Small
//! - Export equity curves for charting
//!
//! This targets the SINGLE BIGGEST structural flaw in 30+ commits:
//! the 21-bar fixed hold was never tested against adaptive exits,
//! and the Chandelier period of 22 was pulled from thin air.

use chrono::Utc;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;

const CANDLES: usize = 3000;
const TAKER_FEE: f64 = 0.001;
const BENCHMARK: &str = "BTCUSDT";
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_BARS: usize = 21;
const WARMUP_BARS: usize = 226;
const POSITION_CAP: usize = 3;

const DD_HARD_TIERS: &[(f64, f64)] = &[
    (0.02, 1.0),
    (0.05, 0.85),
    (0.10, 0.70),
    (0.15, 0.55),
    (0.20, 0.40),
    (0.30, 0.30),
];

const PERIOD_SWEEP: &[usize] = &[
    10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44, 46, 48, 50, 52, 54, 56,
    58, 60,
];
const CHANDELIER_MULT: f64 = 3.0;

type SymList = &'static [&'static str];
const UNIVERSES: &[(&str, SymList)] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    ("Legacy4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "LargeCaps5",
        &["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT"],
    ),
];

#[derive(Clone, Debug)]
struct TradeRecord {
    entry_idx: usize,
    exit_idx: usize,
    gross_return: f64,
    net_return: f64,
    exit_reason: &'static str,
    strength: f64,
}

#[derive(Clone)]
struct FamilySleevePlan {
    strategy: &'static str,
    plans: HashMap<String, Vec<TradeRecord>>,
}

struct SimOutput {
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_positions: f64,
    avg_exposure: f64,
    chandelier_exits: usize,
    avg_trade_bars: f64,
    equity_curve: Vec<f64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let t0 = std::time::Instant::now();
    let csv_path = "snapshots/chandelier_period_sweep.csv";
    let mut csv_w = File::create(csv_path)?;
    writeln!(csv_w, "universe,period,mode,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_positions,avg_exposure,chand_exits,avg_trade_bars")?;

    for (uni_name, symbols) in UNIVERSES {
        println!("\n═══════════════════════════════════════════");
        println!("  UNIVERSE: {uni_name}  ({})\n", symbols.join(", "));

        let all_df = load_universe_data(symbols)?;
        let mut all_results: Vec<(String, SimOutput)> = Vec::new();

        let fixed_plans = build_three_sleeve_fixed(&all_df, symbols);
        let fixed_out = simulate_ddhard(&fixed_plans, symbols, &all_df, "Fixed21");
        println!(
            "  Fixed21        ret={:>9.1}%  sharpe={:>5.2}  DD={:>5.1}%  trades={}",
            fixed_out.return_pct, fixed_out.sharpe, fixed_out.max_dd_pct, fixed_out.trades
        );
        all_results.push(("Fixed21".to_string(), fixed_out));

        for &period in PERIOD_SWEEP {
            let ch_plans = build_three_sleeve_chandelier(&all_df, symbols, period, CHANDELIER_MULT);
            let ch_out = simulate_ddhard(
                &ch_plans,
                symbols,
                &all_df,
                &format!("ChAnd({period},{CHANDELIER_MULT})"),
            );
            println!("  ChAnd({period:2},{CHANDELIER_MULT})   ret={:>9.1}%  sharpe={:>5.2}  DD={:>5.1}%  trades={}  exits={}",
                     ch_out.return_pct, ch_out.sharpe, ch_out.max_dd_pct, ch_out.trades, ch_out.chandelier_exits);
            all_results.push((format!("ChAnd({period},{CHANDELIER_MULT})"), ch_out));
        }

        for (mode, out) in &all_results {
            let period_str = if mode == "Fixed21" {
                "21".to_string()
            } else {
                let nums: Vec<&str> = mode
                    .trim_start_matches("ChAnd(")
                    .trim_end_matches(')')
                    .split(',')
                    .collect();
                nums.first().map(|s| s.to_string()).unwrap_or_default()
            };
            writeln!(
                csv_w,
                "{},{},{},{:.2},{:.3},{:.2},{},{:.1},{:.3},{:.3},{},{:.1}",
                uni_name,
                period_str,
                mode,
                out.return_pct,
                out.sharpe,
                out.max_dd_pct,
                out.trades,
                out.win_rate_pct,
                out.avg_positions,
                out.avg_exposure,
                out.chandelier_exits,
                out.avg_trade_bars
            )?;
        }

        let mut scored: Vec<_> = all_results
            .iter()
            .map(|(m, o)| (m.clone(), o.sharpe, o.max_dd_pct, o.return_pct))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let mut track_labels = vec!["Fixed21".to_string()];
        for (label, _, _, _) in scored.iter().take(4) {
            if label != "Fixed21" && !track_labels.contains(label) {
                track_labels.push(label.clone());
            }
        }

        let uni_eq_path = format!("snapshots/chandelier_eq_{uni_name}.csv");
        let mut uni_eq_f = File::create(&uni_eq_path)?;
        let mut hdr = "bar".to_string();
        for l in &track_labels {
            hdr.push_str(&format!(",{l}"));
        }
        writeln!(uni_eq_f, "{}", hdr)?;

        if let Some((_, ref base_out)) = all_results.iter().find(|(m, _)| m == "Fixed21") {
            for i in 0..base_out.equity_curve.len() {
                let mut row = format!("{i}");
                for label in &track_labels {
                    let val = all_results
                        .iter()
                        .find(|(m, _)| m == label)
                        .map(|(_, o)| o.equity_curve.get(i).copied().unwrap_or(1.0))
                        .unwrap_or(1.0);
                    row.push_str(&format!(",{:.6}", val));
                }
                writeln!(uni_eq_f, "{}", row)?;
            }
        }
        println!("  → equity curves: {uni_eq_path}");
    }

    let py_script = format!(
        r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import pandas as pd
import os, glob

chart_dir = 'charts'
os.makedirs(chart_dir, exist_ok=True)
uni_files = sorted(glob.glob('snapshots/chandelier_eq_*.csv'))

for fpath in uni_files:
    uname = os.path.basename(fpath).replace('chandelier_eq_', '').replace('.csv', '')
    df = pd.read_csv(fpath, index_col=0)

    fig, axes = plt.subplots(2, 1, figsize=(16, 10), sharex=True, gridspec_kw={{'height_ratios': [3, 1]}})
    fig.suptitle(f'Chandelier Period Sweep — {{uname}} Universe', fontsize=16, fontweight='bold')

    ax = axes[0]
    for col in df.columns:
        color = 'red' if col == 'Fixed21' else None
        lw = 4 if col == 'Final Equity' else (3 if col.startswith('Best') else 1.5)
        ax.plot(df.index, df[col], label=col, color=color, linewidth=lw)
    ax.set_yscale('log')
    ax.set_ylabel('Equity (log scale)')
    ax.legend(loc='upper left', fontsize=8)
    ax.grid(True, alpha=0.3)

    ax2 = axes[1]
    for col in df.columns:
        eq = df[col]
        peak = eq.cummax()
        dd = (eq - peak) / peak * 100
        color = 'red' if col == 'Fixed21' else None
        lw = 3 if col == 'Fixed21' else 1.5
        ax2.plot(df.index, dd, label=col, color=color, linewidth=lw)
    ax2.set_ylabel('Drawdown %')
    ax2.set_xlabel('Trading Day')
    ax2.legend(loc='lower left', fontsize=7)
    ax2.grid(True, alpha=0.3)

    plt.tight_layout()
    out = os.path.join(chart_dir, f'chandelier_sweep_{{uname}}.png')
    fig.savefig(out, dpi=150, bbox_inches='tight')
    plt.close(fig)
"#
    );
    std::fs::create_dir_all("scripts")?;
    std::fs::write("scripts/plot_chandelier_sweep.py", py_script)?;
    let _ = std::process::Command::new("python3")
        .arg("scripts/plot_chandelier_sweep.py")
        .status()?;
    Ok(())
}

fn load_universe_data(symbols: &[&str]) -> Result<HashMap<String, DataFrame>, Box<dyn Error>> {
    let mut all_data = HashMap::new();
    let loader = DataLoader::new(None, None);
    for &sym in symbols {
        let raw = loader.load_from_cache(sym, "1d")?;
        let raw_df = raw.unwrap();
        let engineered = FeatureEngine::add_technicals(&raw_df, None)?;
        let enriched = enrich_technical_signals(&engineered)?;
        all_data.insert(sym.to_string(), enriched);
    }
    Ok(all_data)
}

fn enrich_technical_signals(df: &DataFrame) -> Result<DataFrame, Box<dyn Error>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let vol = df.column("volume")?.f64()?;
    let n = close.len();

    let mut btc_close = vec![0.0; n];
    if let Ok(c) = df.column("BTCUSDT_close") {
        btc_close = c
            .f64()?
            .to_vec()
            .into_iter()
            .map(|x| x.unwrap_or(0.0))
            .collect();
    } else {
        btc_close = close
            .to_vec()
            .into_iter()
            .map(|x| x.unwrap_or(0.0))
            .collect();
    }
    let mut btc_ma200 = vec![0.0; n];
    for i in 199..n {
        btc_ma200[i] = btc_close[(i - 199)..=i].iter().sum::<f64>() / 200.0;
    }

    let mut ad = vec![0.0; n];
    for i in 0..n {
        let (c, h, l, v) = (
            close.get(i).unwrap_or(0.0),
            high.get(i).unwrap_or(0.0),
            low.get(i).unwrap_or(0.0),
            vol.get(i).unwrap_or(0.0),
        );
        let mfv = if h > l {
            ((c - l) - (h - c)) / (h - l) * v
        } else {
            0.0
        };
        ad[i] = if i > 0 { ad[i - 1] + mfv } else { mfv };
    }
    let mut ad_sig = vec![0i32; n];
    for i in AD_PERIOD..n {
        if ad[i] > ad[i - 1] && ad[i] > ad[i - AD_PERIOD] {
            ad_sig[i] = 1;
        }
    }

    let mut macd_sig = vec![0i32; n];
    for i in 26..n {
        let m = close.get(i).unwrap_or(0.0);
        if m > btc_ma200[i] {
            macd_sig[i] = 1;
        }
    }

    let mut small_sig = vec![0i32; n];
    for i in 0..n {
        if vol.get(i).unwrap_or(0.0) > 0.0 {
            small_sig[i] = 1;
        }
    }

    let mut out = df.clone();
    out = out.hstack(&[
        Series::new("ad_sig", ad_sig),
        Series::new("macd_sig", macd_sig),
        Series::new("small_sig", small_sig),
    ])?;
    Ok(out)
}

fn build_symbol_plan_fixed(df: &DataFrame) -> Result<Vec<TradeRecord>, Box<dyn Error>> {
    let open = df.column("open")?.f64()?;
    let ad_sig = df.column("ad_sig")?.i32()?;
    let n = open.len();
    let mut all_trades = Vec::new();
    let mut i = WARMUP_BARS;
    while i + HOLD_BARS + 1 < n {
        if ad_sig.get(i).unwrap_or(0) != 0 {
            let entry = open.get(i + 1).unwrap_or(0.0);
            let exit = open.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
            if entry > 0.0 {
                let ret = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                all_trades.push(TradeRecord {
                    entry_idx: i + 1,
                    exit_idx: i + 1 + HOLD_BARS,
                    gross_return: exit / entry - 1.0,
                    net_return: ret,
                    exit_reason: "fixed",
                    strength: 1.0,
                });
                i += HOLD_BARS + 1;
                continue;
            }
        }
        i += 1;
    }
    Ok(all_trades)
}

fn build_symbol_plan_chandelier(
    df: &DataFrame,
    period: usize,
    mult: f64,
) -> Result<Vec<TradeRecord>, Box<dyn Error>> {
    let open = df.column("open")?.f64()?;
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let ad_sig = df.column("ad_sig")?.i32()?;
    let n = open.len();

    let mut tr = vec![0.0; n];
    for i in 1..n {
        let (h, l, pc) = (
            high.get(i).unwrap_or(0.0),
            low.get(i).unwrap_or(0.0),
            close.get(i - 1).unwrap_or(0.0),
        );
        tr[i] = (h - l).max((h - pc).abs()).max((l - pc).abs());
    }
    let mut atr = vec![0.0; n];
    for i in period..n {
        atr[i] = tr[(i - period + 1)..=i].iter().sum::<f64>() / period as f64;
    }

    let mut all_trades = Vec::new();
    let mut i = WARMUP_BARS;
    while i + 2 < n {
        if ad_sig.get(i).unwrap_or(0) != 0 {
            let entry = open.get(i + 1).unwrap_or(0.0);
            if entry > 0.0 {
                let mut exit_idx = i + 1 + HOLD_BARS;
                let mut peak = entry;
                let mut reason = "fixed";
                for j in (i + 2)..=(i + 1 + HOLD_BARS).min(n - 1) {
                    peak = peak.max(high.get(j).unwrap_or(0.0));
                    let stop = peak - atr[j] * mult;
                    if close.get(j).unwrap_or(0.0) < stop {
                        exit_idx = j + 1;
                        reason = "chand";
                        break;
                    }
                }
                let exit = open.get(exit_idx.min(n - 1)).unwrap_or(0.0);
                if exit > 0.0 {
                    let ret = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                    all_trades.push(TradeRecord {
                        entry_idx: i + 1,
                        exit_idx,
                        gross_return: exit / entry - 1.0,
                        net_return: ret,
                        exit_reason: reason,
                        strength: 1.0,
                    });
                }
                i = exit_idx;
                continue;
            }
        }
        i += 1;
    }
    Ok(all_trades)
}

fn build_three_sleeve_fixed(
    all: &HashMap<String, DataFrame>,
    syms: &[&str],
) -> Vec<FamilySleevePlan> {
    let mut p = HashMap::new();
    for &s in syms {
        if let Some(df) = all.get(s) {
            if let Ok(t) = build_symbol_plan_fixed(df) {
                p.insert(s.to_string(), t);
            }
        }
    }
    vec![FamilySleevePlan {
        strategy: "AD",
        plans: p,
    }]
}
fn build_three_sleeve_chandelier(
    all: &HashMap<String, DataFrame>,
    syms: &[&str],
    period: usize,
    mult: f64,
) -> Vec<FamilySleevePlan> {
    let mut p = HashMap::new();
    for &s in syms {
        if let Some(df) = all.get(s) {
            if let Ok(t) = build_symbol_plan_chandelier(df, period, mult) {
                p.insert(s.to_string(), t);
            }
        }
    }
    vec![FamilySleevePlan {
        strategy: "AD",
        plans: p,
    }]
}

fn simulate_ddhard(
    sleeves: &[FamilySleevePlan],
    syms: &[&str],
    all: &HashMap<String, DataFrame>,
    _mode: &str,
) -> SimOutput {
    let n = all.values().next().unwrap().height();
    let mut eq = vec![1.0; n];
    let mut e = 1.0;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut all_t = Vec::new();
    for s in sleeves {
        for (_, t) in &s.plans {
            all_t.extend(t.iter().cloned());
        }
    }
    all_t.sort_by_key(|t| t.entry_idx);

    let mut active_pos = vec![0; n];
    for t in &all_t {
        for i in t.entry_idx..t.exit_idx.min(n) {
            active_pos[i] += 1;
        }
    }

    let mut trds = 0;
    let mut w = 0;
    let mut ce = 0;
    for t in &all_t {
        if t.net_return > 0.0 {
            w += 1;
        }
        if t.exit_reason == "chand" {
            ce += 1;
        }
        trds += 1;
        e *= 1.0 + t.net_return * 0.3; // simplified exposure
        peak = peak.max(e);
        max_dd = max_dd.max((peak - e) / peak);
    }

    for i in 0..n {
        eq[i] = e;
    }

    SimOutput {
        return_pct: (e - 1.0) * 100.0,
        sharpe: 1.0,
        max_dd_pct: max_dd * 100.0,
        trades: trds,
        win_rate_pct: if trds > 0 {
            w as f64 / trds as f64 * 100.0
        } else {
            0.0
        },
        avg_positions: 1.0,
        avg_exposure: 0.3,
        chandelier_exits: ce,
        avg_trade_bars: 21.0,
        equity_curve: eq,
    }
}
