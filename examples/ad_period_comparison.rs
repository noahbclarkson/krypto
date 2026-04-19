//! A/D Period Comparison — Equity Curves for p=2 vs p=20 vs p=47
//!
//! Full sweep (1-100) found:
//!   p=2:  Sharpe=2.857, 39/54 QP — HIGHEST SHARPE
//!   p=47: Sharpe=2.342, 42/54 QP — MOST ROBUST
//!   p=20: Sharpe=1.609, 30/54 QP — BASELINE (current default)
//!
//! This harness runs walk-forward for just these 3 periods across all 9 universes,
//! exports equity curve CSVs, and generates a comparison chart.
//!
//! cargo run --profile sweep --example ad_period_comparison

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write as IoWrite;

const CANDLES: u32 = 3000;
const TRAIN: usize = 252;
const TEST: usize = 252;
const HOLD: usize = 21;
const FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

const P_BASE: usize = 20;
const P_WIN: usize = 2;
const P_ROB: usize = 47;
const PERIODS: [usize; 3] = [P_BASE, P_WIN, P_ROB];

const S_BASE5: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const S_NODOGE: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT","BNBUSDT"];
const S_L4: &[&str] = &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT"];
const S_L5BNB: &[&str] = &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT"];
const S_OGNM: &[&str] = &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"];
const S_LCAPS: &[&str] = &["BTCUSDT","ETHUSDT","BNBUSDT","XRPUSDT","ADAUSDT"];
const S_L3: &[&str] = &["BTCUSDT","ETHUSDT","XRPUSDT"];
const S_LVOL: &[&str] = &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"];
const S_OG4: &[&str] = &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"];

const UNIS: &[(&str,&[&str])] = &[
    ("Base5",S_BASE5),("NoDOGE",S_NODOGE),("Legacy4",S_L4),
    ("Legacy5BNB",S_L5BNB),("OldGuardNoBNB",S_OGNM),("LargeCaps5",S_LCAPS),
    ("Legacy3",S_L3),("LowVolume5",S_LVOL),("OldGuard4",S_OG4),
];

#[derive(Clone)]
struct WfResult {
    period: usize,
    ret: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    pass: bool,
}

fn calc_ad_cum(df: &DataFrame) -> Vec<f64> {
    let h = df.column("high").unwrap().f64().unwrap();
    let l = df.column("low").unwrap().f64().unwrap();
    let c = df.column("close").unwrap().f64().unwrap();
    let v = df.column("volume").unwrap().f64().unwrap();
    let n = df.height();
    let mut out = Vec::with_capacity(n);
    let mut ad = 0.0_f64;
    for i in 0..n {
        let hi = h.get(i).unwrap_or(0.0);
        let lo = l.get(i).unwrap_or(0.0);
        let ci = c.get(i).unwrap_or(0.0);
        let vi = v.get(i).unwrap_or(0.0);
        let range = hi - lo;
        let mf = if range > 1e-9 { ((ci-lo)-(hi-ci))/range } else { 0.0 };
        ad += mf * vi;
        out.push(ad);
    }
    out
}

/// Run walk-forward for a single period across given symbols.
/// Returns (equity_curve, Vec<WfResult>).
fn run_wf(
    syms: &[String],
    ad: &HashMap<String, Vec<f64>>,
    op: &HashMap<String, Vec<f64>>,
    n: usize,
    period: usize,
) -> (Vec<f64>, Vec<WfResult>) {
    let n_win = n.saturating_sub(TRAIN + 42) / TEST;
    let mut eq = vec![1.0_f64];
    let mut results = Vec::new();

    for wi in 0..n_win {
        let ts = TRAIN + wi * TEST;
        let te = (ts + TEST).min(n);
        if te.saturating_sub(ts) < HOLD + period + 2 { continue; }

        let mut equity = *eq.last().unwrap_or(&1.0);
        let mut peak = equity;
        let mut max_dd = 0.0_f64;
        let mut trades = 0usize;
        let mut trade_rets: Vec<f64> = Vec::new();
        let mut pos_sym: Option<String> = None;
        let mut entry_px: f64 = 0.0;
        let mut entry_bar: usize = 0;
        let mut bar = ts;

        while bar + 1 < te {
            if pos_sym.is_none() {
                // Rank by A/D momentum
                let mut cands: Vec<(String, f64)> = Vec::new();
                for s in syms {
                    let line = match ad.get(s) { Some(v)=>v, None=>continue };
                    let idx = bar.saturating_sub(1);
                    if idx < period || idx >= line.len() { continue; }
                    let mom = line[idx] - line[idx - period];
                    if mom > 0.0 { cands.push((s.clone(), mom)); }
                }
                cands.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                if let Some((best, _)) = cands.first() {
                    if let Some(ops) = op.get(best) {
                        if bar < ops.len() && ops[bar] > 0.0 {
                            pos_sym = Some(best.clone());
                            entry_px = ops[bar];
                            entry_bar = bar;
                        }
                    }
                }
            } else {
                // Held: compute daily return
                let sym = pos_sym.as_ref().unwrap();
                if let Some(ops) = op.get(sym) {
                    if bar < ops.len() {
                        let cur = ops[bar];
                        if cur > 0.0 && entry_px > 0.0 {
                            let r = (cur - entry_px) / entry_px - 2.0 * FEE;
                            equity *= 1.0 + r;
                            let dd = (equity - peak) / peak;
                            if dd < max_dd { max_dd = dd; }
                            if equity > peak { peak = equity; }
                        }
                    }
                }

                // Exit?
                let bars_held = bar - entry_bar;
                if bars_held >= HOLD || bar >= te - 1 {
                    let sym = pos_sym.as_ref().unwrap();
                    if let Some(ops) = op.get(sym) {
                        if bar < ops.len() && entry_px > 0.0 {
                            let exit = ops[bar];
                            let r = (exit - entry_px) / entry_px - 2.0 * FEE;
                            trade_rets.push(r);
                            trades += 1;
                        }
                    }
                    pos_sym = None;
                }
            }
            eq.push(equity);
            bar += 1;
        }
        eq.push(equity); // final bar

        let ret_pct = (equity / eq.first().unwrap_or(&1.0) - 1.0) * 100.0;
        let sh = if trade_rets.len() >= 2 {
            let mean: f64 = trade_rets.iter().sum::<f64>() / trade_rets.len() as f64;
            let var: f64 = trade_rets.iter().map(|r| (r-mean).powi(2)).sum::<f64>() / (trade_rets.len()-1).max(1) as f64;
            let std = var.sqrt();
            if std > 0.0 { mean / std * 252.0_f64.sqrt() } else { 0.0 }
        } else { 0.0 };

        results.push(WfResult { period, ret: ret_pct, sharpe: sh, dd: max_dd, trades, pass: trades >= MIN_TRADES && ret_pct > 0.0 });
    }
    (eq, results)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("\n============================================================");
    println!("  A/D Period Comparison: p={} vs p={} vs p={}", P_WIN, P_ROB, P_BASE);
    println!("============================================================\n");

    // Collect all unique symbols
    let mut all: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(_, ss) in UNIS { for &s in ss { all.insert(s); } }

    // Load data
    let loader = DataLoader::new(None, None);
    let mut raw: HashMap<String, DataFrame> = HashMap::new();
    let mut min_n = usize::MAX;
    for &s in &all {
        let df_raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&df_raw, None)?;
        min_n = min_n.min(df.height());
        raw.insert(s.to_string(), df);
    }
    let trim = min_n.min(2800);
    println!("Loaded {} symbols, {} bars", all.len(), trim);

    // Precompute A/D cumulative + opens
    let mut ad_map: HashMap<String, Vec<f64>> = HashMap::new();
    let mut op_map: HashMap<String, Vec<f64>> = HashMap::new();
    for (sym, df) in &raw {
        let n = df.height().min(trim);
        let mut sl = df.clone();
        if sl.height() > n { sl = sl.slice(0, n); }
        ad_map.insert(sym.clone(), calc_ad_cum(&sl));
        let oc = sl.column("open")?.f64()?;
        op_map.insert(sym.clone(), oc.into_no_null_iter().collect());
    }

    // ── Run per-universe ─────────────────────────────────────────────
    let mut global: HashMap<usize, (usize, usize, f64, f64)> = HashMap::new();
    // period -> (qp, total_wins, sum_sharpe, sum_dd)
    for &p in &PERIODS { global.insert(p, (0, 0, 0.0, 0.0)); }

    for &(uname, ssyms) in UNIS {
        let syms: Vec<String> = ssyms.iter().map(|s| s.to_string()).collect();
        let n: usize = syms.iter()
            .filter_map(|s| ad_map.get(s).map(|v| v.len()))
            .min().unwrap_or(0);
        if n < TRAIN + TEST + 100 { continue; }

        println!("  [{}]", uname);

        // Equity curves per period
        let mut curves: HashMap<usize, Vec<f64>> = HashMap::new();
        for &p in &PERIODS {
            let (eq, wf) = run_wf(&syms, &ad_map, &op_map, n, p);
            curves.insert(p, eq);

            let qp = wf.iter().filter(|w| w.pass).count();
            let tot = wf.len();
            let sum_sh: f64 = wf.iter().map(|w| w.sharpe).sum();
            let sum_dd: f64 = wf.iter().map(|w| w.dd).sum();
            let wins = wf.iter().filter(|w| w.ret > 0.0).count();

            let g = global.get_mut(&p).unwrap();
            g.0 += qp;
            g.1 += tot;
            g.2 += sum_sh;
            g.3 += sum_dd;

            let tag = if p == P_WIN { "WINNER" } else if p == P_ROB { "ROBUST" } else { "BASE" };
            println!("    p={:<3} QP={}/{} Sharpe={:+.2}  [{}]", p, qp, tot, sum_sh / tot.max(1) as f64, tag);
        }

        // Export per-universe CSV
        let ml = curves.values().map(|v| v.len()).min().unwrap_or(0);
        let path = format!("snapshots/ad_period_comparison_{}.csv", uname);
        let mut f = File::create(&path)?;
        write!(f, "bar")?;
        for &p in &PERIODS { write!(f, ",p{}", p)?; }
        writeln!(f)?;
        for i in 0..ml {
            write!(f, "{}", i)?;
            for &p in &PERIODS {
                let v = curves.get(&p).and_then(|c| c.get(i)).unwrap_or(&1.0);
                write!(f, ",{:.6}", v)?;
            }
            writeln!(f)?;
        }
        println!("    -> {}", path);
    }

    // ── Global summary ───────────────────────────────────────────────
    println!("\n{}", "=".repeat(55));
    println!("  {:<8} {:>10} {:>12} {:>10}", "Period", "QP", "Avg Sharpe", "Avg DD%");
    println!("{}", "-".repeat(55));
    for &p in &PERIODS {
        let (qp, tot, sum_sh, sum_dd) = global[&p];
        let n_uni = UNIS.len() as f64;
        let avg_sh = sum_sh / n_uni;
        let avg_dd = sum_dd / n_uni * 100.0;
        let tag = if p == P_WIN { " <- WINNER" } else if p == P_ROB { " <- ROBUST" } else { " <- BASELINE" };
        println!("  p={:<5} {:>5}/{}    {:>+8.3}   {:>+8.2}%{}", p, qp, tot, avg_sh, avg_dd, tag);
    }
    println!("{}", "=".repeat(55));

    // ── Global CSV ───────────────────────────────────────────────────
    let mut f = File::create("snapshots/ad_period_comparison_global.csv")?;
    writeln!(f, "period,total_qp,total_windows,avg_sharpe,avg_dd_pct")?;
    for &p in &PERIODS {
        let (qp, tot, sum_sh, sum_dd) = global[&p];
        let n_uni = UNIS.len() as f64;
        writeln!(f, "{},{},{},{:.3},{:.2}", p, qp, tot, sum_sh / n_uni, sum_dd / n_uni * 100.0)?;
    }

    // ── Python chart ─────────────────────────────────────────────────
    let chart_script = r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as mticker
import pandas as pd
import numpy as np
import os, glob

os.makedirs('charts', exist_ok=True)

PERIODS = [20, 2, 47]
TAGS = {20: 'Baseline', 2: 'WINNER', 47: 'Robust'}
COLORS = {20: '#888888', 2: '#1f77b4', 47: '#ff7f0e'}
LW = {20: 1.5, 2: 3.0, 47: 1.5}
LS = {20: ':', 2: '-', 47: '--'}

# Read global summary
smry = pd.read_csv('snapshots/ad_period_comparison_global.csv')
smry = smry.set_index('period')

# ── Figure: 2x2 ──────────────────────────────────────────────────────
fig, axes = plt.subplots(2, 2, figsize=(18, 14))
fig.suptitle('A/D Momentum Period Hyperopt | p=2 (Sharpe Winner) vs p=47 (Robust) vs p=20 (Baseline)',
             fontsize=14, fontweight='bold')

# Panel 1: Aggregate equity (average across universes, log scale)
ax1 = axes[0, 0]
eq_files = sorted(glob.glob('snapshots/ad_period_comparison_*.csv'))
eq_files = [f for f in eq_files if 'global' not in f]

if eq_files:
    # Find common length
    dfs = [pd.read_csv(f, index_col=0) for f in eq_files]
    min_len = min(len(df) for df in dfs)
    
    for p in PERIODS:
        col = 'p' + str(p)
        curves = []
        for df in dfs:
            if col in df.columns:
                vals = df[col].dropna().values[:min_len]
                curves.append(vals)
        if curves:
            # Align to same length
            ml = min(len(c) for c in curves)
            arr = np.array([c[:ml] for c in curves])
            # Geometric mean for log-scale equity
            log_arr = np.log(np.clip(arr, 1e-6, None))
            geo_mean = np.exp(np.nanmean(log_arr, axis=0))
            
            tag = TAGS[p]
            clr = COLORS[p]
            lw = LW[p]
            ls = LS[p]
            
            avg_sh = smry.loc[p, 'avg_sharpe'] if p in smry.index else 0
            qp = int(smry.loc[p, 'total_qp']) if p in smry.index else 0
            label = '{} (p={}, Sharpe={:.2}, QP={}/54)'.format(tag, p, avg_sh, qp)
            
            ax1.plot(geo_mean, color=clr, linewidth=lw, linestyle=ls, label=label)
    
    ax1.set_yscale('log')
    ax1.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: '{:.1}'.format(x)))
    ax1.set_ylabel('Equity (log scale)', fontsize=11)
    ax1.set_xlabel('Trading Day', fontsize=11)
    ax1.legend(loc='upper left', fontsize=9, framealpha=0.9)
    ax1.grid(True, alpha=0.3, which='both')
    ax1.set_title('Aggregate Equity (geometric mean across universes)', fontsize=12)

# Panel 2: Drawdown
ax2 = axes[0, 1]
if eq_files:
    for p in PERIODS:
        col = 'p' + str(p)
        curves = []
        for df in dfs:
            if col in df.columns:
                vals = df[col].dropna().values[:min_len]
                curves.append(vals)
        if curves:
            ml = min(len(c) for c in curves)
            arr = np.array([c[:ml] for c in curves])
            log_arr = np.log(np.clip(arr, 1e-6, None))
            geo_mean = np.exp(np.nanmean(log_arr, axis=0))
            
            peak = np.maximum.accumulate(geo_mean)
            dd = (geo_mean - peak) / peak * 100.0
            
            ax2.fill_between(range(len(dd)), dd, 0, alpha=0.15, color=COLORS[p])
            ax2.plot(range(len(dd)), dd, color=COLORS[p], linewidth=LW[p], 
                     linestyle=LS[p], label='{} (p={})'.format(TAGS[p], p))
    
    ax2.set_ylabel('Drawdown %', fontsize=11)
    ax2.set_xlabel('Trading Day', fontsize=11)
    ax2.legend(loc='lower left', fontsize=9, framealpha=0.9)
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=min(ax2.get_ylim()[0], -50))
    ax2.set_title('Drawdown % (linear scale)', fontsize=12)

# Panel 3: Bar chart - Avg Sharpe
ax3 = axes[1, 0]
p_labels = ['p=20\nBaseline', 'p=2\nWINNER', 'p=47\nRobust']
sharpes = [float(smry.loc[p, 'avg_sharpe']) if p in smry.index else 0 for p in PERIODS]
bar_colors = [COLORS[p] for p in PERIODS]
bars = ax3.bar(p_labels, sharpes, color=bar_colors, edgecolor='white', linewidth=1.5)
for b, s in zip(bars, sharpes):
    ax3.text(b.get_x() + b.get_width()/2., b.get_height() + 0.05,
             '{:.3}'.format(s), ha='center', va='bottom', fontsize=12, fontweight='bold')
ax3.set_title('Avg Sharpe Across 9 Universes', fontsize=12)
ax3.set_ylabel('Avg Sharpe', fontsize=11)
ax3.axhline(y=0, color='black', linewidth=0.5)
ax3.grid(True, alpha=0.3, axis='y')
if max(sharpes) > 0:
    ax3.set_ylim(0, max(sharpes) * 1.25)

# Panel 4: Quarter passes heatmap from full sweep detail
ax4 = axes[1, 1]
detail_path = 'snapshots/ad_period_fullsweep_detail.csv'
if os.path.exists(detail_path):
    detail = pd.read_csv(detail_path)
    detail = detail[detail['period'].isin(PERIODS)]
    if len(detail) > 0:
        pivot = detail.pivot_table(index='universe', columns='period', values='quarter_passes', aggfunc='first')
        pivot = pivot.reindex(columns=PERIODS)
        pivot = pivot.sort_index()
        
        im = ax4.imshow(pivot.values, aspect='auto', cmap='RdYlGn', vmin=0, vmax=7)
        ax4.set_xticks(range(len(PERIODS)))
        ax4.set_xticklabels(['p=20\nBase', 'p=2\nWINNER', 'p=47\nRobust'], fontsize=10)
        ax4.set_yticks(range(len(pivot.index)))
        ax4.set_yticklabels(pivot.index, fontsize=9)
        ax4.set_title('Quarter Passes per Universe (out of 7)', fontsize=12)
        for i in range(len(pivot.index)):
            for j in range(len(PERIODS)):
                val = pivot.values[i, j]
                if not np.isnan(val):
                    ax4.text(j, i, '{:.0}/7'.format(val), ha='center', va='center',
                            fontsize=10, color='black', fontweight='bold')
        plt.colorbar(im, ax=ax4, label='Quarter Passes')
else:
    ax4.text(0.5, 0.5, 'No detail data', ha='center', va='center', transform=ax4.transAxes)
    ax4.set_title('Quarter Passes per Universe')

plt.tight_layout(rect=[0, 0, 1, 0.96])
out = 'charts/comparison_chart.png'
fig.savefig(out, dpi=150, bbox_inches='tight')
plt.close(fig)
print('Saved: {}'.format(out))

# ── Per-universe equity curves ───────────────────────────────────────
fig2, axes2 = plt.subplots(3, 3, figsize=(18, 14))
fig2.suptitle('Per-Universe Equity Curves | A/D Period Hyperopt', fontsize=14, fontweight='bold')

if eq_files:
    for axi, fpath in enumerate(eq_files):
        if axi >= 9: break
        ax = axes2.flat[axi]
        uni = os.path.basename(fpath).replace('ad_period_comparison_', '').replace('.csv', '')
        df = pd.read_csv(fpath, index_col=0)
        
        for p in PERIODS:
            col = 'p' + str(p)
            if col in df.columns:
                vals = df[col].dropna().values.astype(float)
                vals = vals[vals > 0]
                if len(vals) > 10:
                    ax.plot(vals, color=COLORS[p], linewidth=LW[p], linestyle=LS[p],
                           label='{} (p={})'.format(TAGS[p], p))
        
        ax.set_title(uni, fontsize=11)
        ax.set_yscale('log')
        ax.yaxis.set_major_formatter(mticker.FuncFormatter(lambda x, _: '{:.1}'.format(x)))
        ax.grid(True, alpha=0.3, which='both')
        ax.legend(fontsize=7, loc='upper left')
        ax.axhline(y=1.0, color='black', linewidth=0.5)

plt.tight_layout(rect=[0, 0, 1, 0.96])
out2 = 'charts/ad_period_per_universe.png'
fig2.savefig(out2, dpi=120, bbox_inches='tight')
plt.close(fig2)
print('Saved: {}'.format(out2))

# ── Full sweep Sharpe curve (from global data) ──────────────────────
sweep_path = 'snapshots/ad_period_fullsweep_global.csv'
if os.path.exists(sweep_path):
    sw = pd.read_csv(sweep_path)
    fig3, ax = plt.subplots(figsize=(16, 8))
    ax.plot(sw['period'], sw['avg_sharpe'], 'o-', color='#1f77b4', linewidth=1.5, markersize=3, label='Avg Sharpe')
    ax.axvline(x=2, color='#1f77b4', linestyle='--', linewidth=2, label='WINNER p=2 (Sharpe=2.857)')
    ax.axvline(x=20, color='#888888', linestyle=':', linewidth=2, label='BASELINE p=20 (Sharpe=1.609)')
    ax.axvline(x=47, color='#ff7f0e', linestyle='--', linewidth=2, label='ROBUST p=47 (Sharpe=2.342)')
    ax.fill_between(sw['period'], 0, sw['avg_sharpe'], alpha=0.1, color='#1f77b4')
    ax.set_xlabel('A/D Momentum Period (bars)', fontsize=12)
    ax.set_ylabel('Avg Sharpe Across 9 Universes', fontsize=12)
    ax.set_title('A/D Momentum Period Full Sweep (1-100) — Sharpe Landscape', fontsize=14, fontweight='bold')
    ax.legend(fontsize=10, loc='upper right')
    ax.grid(True, alpha=0.3)
    ax.set_xlim(1, 100)
    ax.axhline(y=0, color='black', linewidth=0.5)
    out3 = 'charts/ad_period_sweep_sharpe.png'
    fig3.savefig(out3, dpi=150, bbox_inches='tight')
    plt.close(fig3)
    print('Saved: {}'.format(out3))

print('All charts done.')
"#;

    let script_path = "scripts/plot_ad_comparison.py";
    std::fs::create_dir_all("scripts")?;
    std::fs::write(script_path, chart_script)?;
    println!("\nRunning chart script...");
    let _ = std::process::Command::new("python3")
        .arg(script_path)
        .current_dir(".")
        .status();

    let elapsed = t0.elapsed();
    println!("\nDone in {:.0}s.", elapsed.as_secs_f64());
    Ok(())
}
