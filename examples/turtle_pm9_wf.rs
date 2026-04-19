//! 9-universe walk-forward validation of P=15/M=1.50 vs P=20/M=2.15
//! Quick 2-config comparison across all 9 universes
use anyhow::Result;
use krypto::data::loader::DataLoader;
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
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CONFIGS: &[(usize, f64)] = &[
    (20, 2.15),  // current default
    (15, 1.50),  // 2D sweep winner
];

const CSV_OUT: &str = "snapshots/pm9_wf_results.csv";

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>, }

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, i: usize) -> f64 {
    if i < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for j in (i + 1 - p)..=i {
        let hh = h.get(j).copied().unwrap_or(0.0);
        let ll = l.get(j).copied().unwrap_or(0.0);
        let cc = c.get(j.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((hh - ll).max((hh - cc).abs()).max((ll - cc).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / p as f64
}

fn rolling_avg(v: &[f64], w: usize, i: usize) -> f64 {
    if i < w { return *v.get(i).unwrap_or(&0.0); }
    v[i + 1 - w..=i].iter().sum::<f64>() / w as f64
}

fn turtle_signal(c: &[f64], ep: usize, i: usize) -> bool {
    if i < ep + 1 { return false; }
    let start = i + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for j in start..i { if let Some(&x) = c.get(j) { mx = mx.max(x); } }
    if let Some(&x) = c.get(i) { x > mx } else { false }
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let mn: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
    let sd = (rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);
    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "universe,window,p,m,avg_sharpe,avg_ret_pct,total_trades,pass_rate")?;

    for (uname, syms) in UNIVERSES {
        println!("\nLoading {uname}...");
        let mut sd: HashMap<String, SymData> = HashMap::new();
        for sym in *syms {
            match loader.fetch_with_cache(sym, "1d", CANDLES).await {
                Ok(c) => {
                    let n = c.height().min(CANDLES as usize);
                    let close: Vec<f64> = c.column("close")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let high: Vec<f64> = c.column("high")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let low:  Vec<f64> = c.column("low")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let vol:  Vec<f64> = c.column("volume")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    sd.insert(sym.to_string(), SymData { close, high, low, vol });
                }
                Err(e) => { eprintln!("  {sym} failed: {e}"); }
            }
        }
        let n = sd.values().next().map(|x| x.close.len()).unwrap_or(0);
        let n_win = n / TEST_BARS;

        for &(cp, cm) in CONFIGS {
            let mut tot_sh = 0.0_f64; let mut tot_ret = 0.0_f64;
            let mut tot_tr = 0usize; let mut passes = 0usize;
            for w in 0..n_win {
                let te = TRAIN_BARS + w * TEST_BARS;
                let ts = te; let tn = (te + TEST_BARS).min(n);
                if tn - ts < 50 { continue; }

                let symbols: Vec<String> = sd.keys().cloned().collect();
                let mut equity = 1.0_f64; let mut wins = 0usize; let mut trades = 0usize;
                let mut daily_rets = Vec::new();

                for bar in ts..tn {
                    let mut scores: Vec<(&str,f64)> = Vec::new();
                    for sym in &symbols {
                        if let Some(d) = sd.get(sym) {
                            if bar >= d.close.len() { continue; }
                            let rv = rolling_avg(&d.vol, VOL_LOOKBACK, bar);
                            let px = d.close.get(bar).copied().unwrap_or(0.0);
                            let dv = rv * px;
                            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
                        }
                    }
                    scores.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap());
                    let top: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s,_)| s.to_string()).collect();
                    if top.is_empty() { daily_rets.push(0.0); continue; }

                    let mut entered = false;
                    for sym in &top {
                        if entered { break; }
                        if let Some(d) = sd.get(sym) {
                            if bar >= TURTLE_ENTRY+1 && bar < d.close.len() && turtle_signal(&d.close, TURTLE_ENTRY, bar) {
                                let ep = d.close[bar] * (1.0 - TAKER_FEE);
                                let ebn = bar + 1; let ns = d.close.len();
                                let mut hhc = d.high[ebn]; let mut hht = d.high[ebn];
                                let mb = (ebn + HOLD_MAX).min(ns.saturating_sub(1)); let mut xb = mb;
                                for b in ebn..=mb.min(ns-1) {
                                    hhc = hhc.max(d.high[b]);
                                    let ac = atr_at(&d.high,&d.low,&d.close,cp,b);
                                    let tc = hhc - cm * ac;
                                    hht = hht.max(d.high[b]);
                                    let at = atr_at(&d.high,&d.low,&d.close,TURTLE_ATR_PERIOD,b);
                                    let tt = hht - TURTLE_ATR_MULT * at;
                                    if d.close[b] < tc || d.close[b] < tt { xb = b; break; }
                                }
                                if let Some(&xp) = d.close.get(xb) {
                                    let ex = xp * (1.0 - TAKER_FEE);
                                    let gr = ex/ep - 1.0;
                                    wins += if gr > 0.0 { 1 } else { 0 };
                                    trades += 1; equity *= 1.0 + gr;
                                    daily_rets.push(gr);
                                }
                                entered = true;
                            }
                        }
                    }
                    if !entered { daily_rets.push(0.0); }
                }

                let ret = (equity - 1.0) * 100.0;
                let sh = annualised_sharpe(&daily_rets);
                tot_sh += sh; tot_ret += ret; tot_tr += trades;
                if trades >= MIN_TRADES && sh > 0.0 { passes += 1; }
            }

            let n_v = n_win.max(1);
            writeln!(csv, "{},P={}/M={:.2},{:.4},{:.4},{},{:.2}", uname, cp, cm, tot_sh/n_v as f64, tot_ret/n_v as f64, tot_tr, passes as f64/n_v as f64*100.0)?;
            println!("  P={cp} M={:.2}: Sharpe={:.4} Ret={:.2}% Trades={tot_tr} Pass={:.0}%", cm, tot_sh/n_v as f64, tot_ret/n_v as f64, passes as f64/n_v as f64*100.0);
        }
    }

    println!("\nDone in {:.1}s. Results: {}", start.elapsed().as_secs_f64(), CSV_OUT);
    Ok(())
}
