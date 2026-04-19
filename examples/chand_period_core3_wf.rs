//: CHAND_PERIOD Sweep - 3 Core Universes, 24 CP Values
//: Production params: EP=21, ATR_P=24, ATR_M=2.0, CHAND_M=1.50, HM=45, CAP=3
//: Grid: coarse 5-100 step 5 + fine 12-23 step 1 (24 total)
//: Exports: per-window results

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const CHAND_MULT: f64 = 1.50;
const VOL_LOOKBACK: usize = 2;

// Smart grid: coarse full-range + fine around P=15
const CHAND_PERIODS: &[usize] = &[
     5, 10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100,  // coarse
    12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23,            // fine
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
];

const CSV_OUT: &str = "snapshots/chand_period_core3_wf.csv";

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

#[inline]
fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut s = 0.0_f64;
    for i in (idx + 1 - p)..=idx {
        let hi = *h.get(i).unwrap_or(&0.0);
        let li = *l.get(i).unwrap_or(&0.0);
        let c0 = *c.get(i.saturating_sub(1)).unwrap_or(&0.0);
        let tr = (hi - li).max((hi - c0).abs()).max((li - c0).abs());
        s += tr;
    }
    s / p as f64
}

#[inline]
fn rol_avg(v: &[f64], w: usize, idx: usize) -> f64 {
    if idx + 1 < w { return *v.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - w;
    v[start..=idx].iter().sum::<f64>() / w as f64
}

#[inline]
fn turtle_sig(c: &[f64], ep: usize, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    c.get(idx).map(|&cv| cv > mx).unwrap_or(false)
}

#[inline]
fn ann_sharpe(r: &[f64]) -> f64 {
    if r.len() < 2 { return 0.0; }
    let mn: f64 = r.iter().sum::<f64>() / r.len() as f64;
    let sd = (r.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / r.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

#[inline]
fn max_dd(e: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut mxdd = 0.0_f64;
    for &eq in e {
        if eq > peak { peak = eq; }
        let dd = (peak - eq) / peak;
        if dd > mxdd { mxdd = dd; }
    }
    mxdd * 100.0
}

fn run_sim(
    sd: &HashMap<String, SymData>,
    syms: &[String],
    ts: usize,
    te: usize,
    cp: usize,
) -> (f64, f64, f64, usize, f64) {
    let mut eq = 1.0_f64;
    let mut wins = 0usize;
    let mut tot = 0usize;
    let mut rets = Vec::new();

    let mut bar = ts;
    while bar + 2 < te {
        let mut sc: Vec<(&str, f64)> = Vec::new();
        for sym in syms {
            if let Some(d) = sd.get(sym) {
                if bar >= d.close.len() { continue; }
                let rv = rol_avg(&d.vol, VOL_LOOKBACK, bar);
                let px = d.close.get(bar).copied().unwrap_or(0.0);
                let dv = rv * px;
                sc.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        sc.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top: Vec<String> = sc.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if top.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top {
            if let Some(d) = sd.get(sym) {
                if bar >= EP + 1 && bar < d.close.len() {
                    if turtle_sig(&d.close, EP, bar) {
                        let epx = d.close[bar];
                        let epxn = bar + 1;
                        let n = d.close.len();
                        let mb = (epxn + HOLD_MAX).min(n.saturating_sub(1));
                        let mut hhc = d.high[epxn.min(n.saturating_sub(1))];
                        let mut hht = d.high[epxn.min(n.saturating_sub(1))];
                        let mut exb = mb;
                        for b in epxn..=mb {
                            if b >= n { break; }
                            hhc = hhc.max(d.high[b]);
                            let atr_c = atr_at(&d.high, &d.low, &d.close, cp, b);
                            let trl_c = hhc - CHAND_MULT * atr_c;
                            hht = hht.max(d.high[b]);
                            let atr_t = atr_at(&d.high, &d.low, &d.close, TURTLE_ATR_PERIOD, b);
                            let trl_t = hht - TURTLE_ATR_MULT * atr_t;
                            if d.close[b] < trl_c || d.close[b] < trl_t { exb = b; break; }
                        }
                        if let Some(&xpx) = d.close.get(exb) {
                            let xret = (xpx * (1.0 - TAKER_FEE)) / (epx * (1.0 - TAKER_FEE)) - 1.0;
                            wins += if xret > 0.0 { 1 } else { 0 };
                            tot += 1;
                            eq *= 1.0 + xret;
                            rets.push(xret);
                        }
                        entered = true;
                        break;
                    }
                }
            }
        }
        bar += 1;
    }

    let ret = (eq - 1.0) * 100.0;
    let shr = ann_sharpe(&rets);
    let mdd = max_dd(&[eq]); // single equity point
    let wr = if tot > 0 { wins as f64 / tot as f64 * 100.0 } else { 0.0 };
    (ret, shr, mdd, tot, wr)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_PERIOD Sweep: 3 Core Universes, 24 CP Values ====");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, ss) in UNIVERSES { for &s in *ss { all_syms.insert(s.to_string()); } }

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut mn = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { mn = mn.min(df.height()); cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("WARN: {} failed: {}", sym, e); }
        }
    }
    let n = mn.min(2800);
    let mut sm: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = cache.get(sym) {
            let nn = df.height().min(n);
            sm.insert(sym.clone(), SymData {
                close: df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(nn).collect(),
                high:  df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(nn).collect(),
                low:   df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(nn).collect(),
                vol:   df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(nn).collect(),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sm.len(), n);

    let mut res: Vec<(String, usize, usize, f64, f64, f64, usize, f64, i32)> = Vec::new();

    for &cp in CHAND_PERIODS {
        eprint!("CP={:3} ", cp);
        for &(lbl, ss) in UNIVERSES {
            let syms: Vec<String> = ss.iter().map(|s| s.to_string()).collect();
            let mw = {
                let lens: Vec<usize> = syms.iter().filter_map(|s| sm.get(s).map(|d| d.close.len())).collect();
                lens.iter().fold(usize::MAX, |a, &b| a.min(b)).saturating_sub(TRAIN_BARS + TEST_BARS)
            };
            for wi in 0..mw {
                let ts = wi;
                let te = (wi + TRAIN_BARS + TEST_BARS).min(
                    syms.iter().filter_map(|s| sm.get(s).map(|d| d.close.len())).min().unwrap_or(0)
                );
                if te <= ts + 10 { break; }
                let (ret, shr, mdd, tot, wr) = run_sim(&sm, &syms, ts, te, cp);
                res.push((lbl.to_string(), cp, wi, ret, shr, mdd, tot, wr,
                    if tot >= MIN_TRADES { 1 } else { 0 }));
            }
        }
        eprintln!();
    }

    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "universe,chand_p,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
        for r in &res {
            writeln!(f, "{},{},{},{},{},{},{},{},{}",
                r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8)?;
        }
    }

    eprintln!("\nRuntime: {}s", t0.elapsed().as_secs_f64());
    eprintln!("Saved: {}", CSV_OUT);

    // Summary by CP
    let mut smry: std::collections::HashMap<usize, (f64, f64, usize, usize, usize)> =
        std::collections::HashMap::new();
    for r in &res {
        let e = smry.entry(r.1).or_insert((0.0, 0.0, 0, 0, 0));
        e.0 += r.3; // sum return
        e.1 += r.4; // sum sharpe
        e.2 += r.6; // sum trades
        e.3 += if r.8 == 1 { 1 } else { 0 }; // passes
        e.4 += 1; // total windows
    }
    let tot_w = UNIVERSES.len() * (n.saturating_sub(TRAIN_BARS + TEST_BARS)).min(9);

    let mut srt: Vec<_> = smry.iter().collect();
    srt.sort_by(|a, b| {
        let as_ = a.1.1 / a.1.2.max(1) as f64;
        let bs_ = b.1.1 / b.1.2.max(1) as f64;
        bs_.partial_cmp(&as_).unwrap()
    });

    eprintln!("\n=== CHAND_PERIOD Ranking (3 Core Universes) ===");
    eprintln!("{:>4}  {:>10}  {:>7}  {:>7}  {:>6}", "CP", "AvgSharpe", "Trades", "PassRate", "AvgRet%");
    eprintln!("{}", "-".repeat(40));
    for (&cp, &(sr, ss, st, sp, cnt)) in srt.iter() {
        let avs = ss / st.max(1) as f64;
        let pr = sp as f64 / tot_w as f64 * 100.0;
        let avr = sr / cnt as f64;
        eprintln!("{:4}:  {:10.3}  {:7}  {:6.0}%  {:6.1}%",
            cp, avs, st, pr, avr);
    }

    Ok(())
}
