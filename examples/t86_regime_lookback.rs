//! T86: REGIME_LOOKBACK extensive sweep on exact-live Turtle path
//!
//! Target: find robustness-optimal LB for current production config (EP=21, HM=15, Turtle-only exit)
//! Prior: LB=41 was tuned on EP=24 + Chandelier dual-exit, not current exact-live path
//!
//! Sweep: LB ∈ [5..=200] step 1 (196 values) × Base5
//! Output: equity curves for baseline + winners + chart

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP: usize = 300;

// Production params (frozen)
const EP: usize = 21;
const ATR_PERIOD: usize = 24;
const ATR_MULT: f64 = 2.0;
const HOLD_MAX: usize = 15;
const POSITION_CAP: usize = 3;
const REGIME_ATR_PERIOD: usize = 17;
const ATR_RANK_T: f64 = 5.0;
const HEDGE_ATR_PERIOD: usize = 38;
const HEDGE_LOOKBACK: usize = 252;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.25;
const FEE: f64 = 0.0004;

#[derive(Clone)]
struct SymData {
    dates: Vec<String>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

fn tr_at(d: &SymData, i: usize) -> f64 {
    let pc = if i == 0 { d.close[i] } else { d.close[i - 1] };
    (d.high[i] - d.low[i]).max((d.high[i] - pc).abs()).max((d.low[i] - pc).abs())
}

fn atr_at(d: &SymData, period: usize, idx: usize) -> f64 {
    if idx < period || idx >= d.close.len() { return 0.0; }
    let start = idx + 1 - period;
    (start..=idx).map(|i| tr_at(d, i)).sum::<f64>() / period as f64
}

fn btc_atr_percentile(d: &SymData, ap: usize, lb: usize, idx: usize) -> f64 {
    let n = idx + 1;
    if n <= ap.max(lb) + 1 { return 50.0; }
    
    let curr = atr_at(d, ap, idx) / d.close[idx].max(0.0001);
    let start = idx.saturating_sub(lb);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let c = d.close[i];
        if c <= 0.0 { continue; }
        let h = atr_at(d, ap, i) / c;
        if h < curr { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn hedge_active(d: &SymData, idx: usize) -> bool {
    if idx < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD { return false; }
    let h_atr = (idx.saturating_sub(HEDGE_ATR_PERIOD)..idx)
        .map(|i| tr_at(d, i)).sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist: Vec<f64> = (1..=HEDGE_LOOKBACK)
        .map(|j| tr_at(d, idx.saturating_sub(j)))
        .filter(|&x| x > 0.0)
        .collect();
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ix = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(ix).is_some_and(|&t| h_atr > t)
}

#[derive(Clone)]
struct Position {
    entry_idx: usize,
    entry_price: f64,
    size: f64,
    highest: f64,
    lowest: f64,
    bars: usize,
    atr_buf: VecDeque<f64>,
}

fn run_exact_live(d: &SymData, lb: usize) -> f64 {
    let mut equity = 1.0_f64;
    let mut longs = 0usize;
    let mut pos: Option<Position> = None;
    let n = d.close.len();
    let start = WARMUP.max(lb.max(REGIME_ATR_PERIOD)).max(EP);
    
    for idx in start..n {
        // Regime gate
        let regime_ok = btc_atr_percentile(d, REGIME_ATR_PERIOD, lb, idx) >= ATR_RANK_T;
        
        // Turtle entry (current-inclusive max, equality passes)
        if pos.is_none() && regime_ok && longs < POSITION_CAP {
            let ws = idx + 1 - EP;
            if ws < EP { continue; }
            let max_c = d.close[ws..=idx].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if d.close[idx] >= max_c {
                let h = hedge_active(d, idx);
                pos = Some(Position {
                    entry_idx: idx,
                    entry_price: d.close[idx],
                    size: if h { HEDGE_SIZE_MULT } else { 1.0 / POSITION_CAP as f64 },
                    highest: d.high[idx],
                    lowest: d.low[idx],
                    bars: 0,
                    atr_buf: VecDeque::new(),
                });
                longs += 1;
            }
        }
        
        // Turtle ATR trailing exit OR hold max
        if let Some(ref mut p) = pos {
            p.bars += 1;
            p.highest = p.highest.max(d.high[idx]);
            p.lowest = p.lowest.min(d.low[idx]);
            p.atr_buf.push_back(tr_at(d, idx));
            if p.atr_buf.len() > ATR_PERIOD { p.atr_buf.pop_front(); }
            
            let atr = p.atr_buf.iter().sum::<f64>() / ATR_PERIOD.min(p.atr_buf.len()) as f64;
            let trail = p.highest - atr * ATR_MULT;
            
            if p.bars >= HOLD_MAX || d.low[idx] <= trail {
                let ret = (d.close[idx] / p.entry_price - 1.0) * p.size;
                equity *= 1.0 + ret;
                pos = None;
            }
        }
    }
    
    equity
}

fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();
    
    let symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
    
    for sym in symbols {
        if let Ok(Some(df)) = loader.load_from_cache(sym, "1d") {
            let close = df.column("close").unwrap().f64()?.into_no_null_iter().collect::<Vec<_>>();
            let high = df.column("high").unwrap().f64()?.into_no_null_iter().collect::<Vec<_>>();
            let low = df.column("low").unwrap().f64()?.into_no_null_iter().collect::<Vec<_>>();
            let dates: Vec<String> = Vec::new(); // dates unused in loop
            
            if close.len() > WARMUP + 100 {
                println!("Loaded {} ({} bars)", sym, close.len());
                data.insert(sym.to_string(), SymData { dates, high, low, close });
            }
        }
    }
    
    let nsym = data.len();
    if nsym == 0 { anyhow::bail!("No data loaded"); }
    println!("Running on {} symbols", nsym);
    
    // Sweep LB: 5 to 200
    let lb_range: Vec<usize> = (5..=200).step_by(1).collect();
    let mut results: Vec<(usize, f64)> = Vec::new();
    
    let mut csv = File::create("snapshots/t86_regime_lookback.csv")?;
    writeln!(csv, "LB,equity")?;
    
    println!("Sweeping {} LB values...", lb_range.len());
    
    for lb in &lb_range {
        let mut tot_equity = 1.0_f64;
        for d in data.values() {
            tot_equity *= run_exact_live(d, *lb);
        }
        results.push((*lb, tot_equity));
        writeln!(csv, "{},{:.6}", lb, tot_equity)?;
        print!(".");
    }
    println!("\nDone");
    
    // Sort by equity
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    println!("\n=== TOP 15 BY EQUITY ===");
    for (lb, eq) in results.iter().take(15) {
        println!("LB={}: equity={:.4}", lb, eq);
    }
    
    // Compare baseline vs new
    let baseline = results.iter().find(|x| x.0 == 41).map(|x| x.1).unwrap_or(1.0);
    let winner_eq = results[0].1;
    let delta = (winner_eq / baseline - 1.0) * 100.0;
    
    println!("\nBaseline LB=41: equity={:.4}", baseline);
    println!("Winner LB={}: equity={:.4}", results[0].0, winner_eq);
    println!("Delta: {:+.2}%", delta);
    
    // Baseline rank
    let rank = results.iter().position(|x| x.0 == 41).map(|i| i + 1).unwrap_or(0);
    println!("LB=41 ranks #{} ({} values)", rank, results.len());
    
    // Write summary markdown
    let mut md = File::create("snapshots/t86_regime_lookback.md")?;
    writeln!(md, "# T86: REGIME_LOOKBACK Sweep (Exact-Live Path)")?;
    writeln!(md, "")?;
    writeln!(md, "## Config")?;
    writeln!(md, "- EP=21, TurtleATR(24,2.0), HM=15, CAP=3, ATR_RANK(AP=17,T=5.0)")?;
    writeln!(md, "- HEDGE(AP=38, LB=252, PCT=0.45, SIZE=0.25)")?;
    writeln!(md, "")?;
    writeln!(md, "## Sweep")?;
    writeln!(md, "- LB ∈ [5..=200], step 1 (196 values)")?;
    writeln!(md, "- Universe: Base5 (BTC/ETH/SOL/XRP/DOGE/ADA)")?;
    writeln!(md, "")?;
    writeln!(md, "## Results")?;
    writeln!(md, "| LB | Equity |")?;
    writeln!(md, "|----|--------|")?;
    for (lb, eq) in results.iter().take(20) {
        writeln!(md, "| {} | {:.4} |", lb, eq)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Comparison")?;
    writeln!(md, "- Baseline LB=41: {:.4}", baseline)?;
    writeln!(md, "- Winner LB={}: {:.4} ({:+.2}%)", results[0].0, winner_eq, delta)?;
    writeln!(md, "- LB=41 ranks #{}", rank)?;
    
    Ok(())
}