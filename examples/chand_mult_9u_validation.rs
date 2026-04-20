//! CHAND_MULT 9-Universe Validation — Top configs from sweep
//!
//! Validates M ∈ {1.50, 2.00, 2.25, 2.50, 3.00} across all 9 universes
//! to find the genuinely robust optimum.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

const M_VALUES: &[f64] = &[1.50, 2.00, 2.25, 2.50, 3.00];

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

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx+1-period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h-l).max((h-c0).abs()).max((l-c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_dv(close: &[f64], vol: &[f64], lookback: usize, bar: usize) -> f64 {
    if bar < lookback.saturating_sub(1) { return close.get(bar).copied().unwrap_or(0.0) * vol.get(bar).copied().unwrap_or(0.0); }
    let start = bar + 1 - lookback;
    (start..=bar).map(|i| close.get(i).copied().unwrap_or(0.0) * vol.get(i).copied().unwrap_or(0.0)).sum::<f64>() / lookback as f64
}

fn run_window(sym_data: &HashMap<String, SymData>, symbols: &[&str], train_end: usize, test_end: usize, chand_mult: f64) -> (usize, f64, f64, f64, f64) {
    let warmup = CHAND_PERIOD.max(TURTLE_ATR_PERIOD).max(TURTLE_ENTRY) + TURTLE_ATR_PERIOD;
    let test_start = train_end;
    let n = test_end.min(sym_data.values().next().map(|s| s.close.len()).unwrap_or(0));

    let mut scores: Vec<(&str, f64)> = symbols.iter().filter_map(|s| {
        sym_data.get(*s).and_then(|sd| {
            if test_start < sd.close.len() {
                let dv = rolling_dv(&sd.close, &sd.vol, VOL_LOOKBACK, test_start);
                Some((*s, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }))
            } else { None }
        })
    }).collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top: Vec<&str> = scores.iter().take(POSITION_CAP).map(|(s, _)| *s).collect();

    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;

    let mut bar = test_start.max(warmup);
    while bar < n {
        let mut entered = false;
        for &sym in &top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let start_idx = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start_idx..bar { if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); } }
                    let curr = sd.close.get(bar).copied().unwrap_or(0.0);
                    if curr > max_close {
                        let entry = sd.close[bar] * (1.0 - TAKER_FEE);
                        let next_bar = bar + 1;
                        let max_hold = (next_bar + HOLD_MAX).min(sd.close.len().saturating_sub(1));
                        let mut exit_bar = max_hold;
                        let mut hh_c = sd.high.get(next_bar).copied().unwrap_or(0.0);
                        let mut hh_t = sd.high.get(next_bar).copied().unwrap_or(0.0);
                        for b in next_bar..=max_hold {
                            hh_c = hh_c.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let trail_c = hh_c - chand_mult * atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            hh_t = hh_t.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let trail_t = hh_t - TURTLE_ATR_MULT * atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            if sd.close.get(b).copied().unwrap_or(0.0) < trail_c || sd.close.get(b).copied().unwrap_or(0.0) < trail_t {
                                exit_bar = b; break;
                            }
                        }
                        let exit = sd.close.get(exit_bar).copied().unwrap_or(sd.close[bar]) * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        equity *= 1.0 + gross_ret;
                        peak = peak.max(equity);
                        max_dd = max_dd.min(equity / peak - 1.0);
                        rets.push(gross_ret);
                        trades += 1;
                        bar = exit_bar + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let sharpe = if rets.len() >= 2 {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let std = (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len()-1) as f64).sqrt();
        if std > 1e-10 { mean / std * 252f64.sqrt() } else { 0.0 }
    } else { 0.0 };
    let wins = rets.iter().filter(|&&r| r > 0.0).count();
    let wr = if !rets.is_empty() { wins as f64 / rets.len() as f64 * 100.0 } else { 0.0 };

    (trades, equity - 1.0, sharpe, max_dd, wr)
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("=== CHAND_MULT 9-Universe Validation ===");

    let loader = DataLoader::new(None, None);
    let mut all_data: HashMap<String, SymData> = HashMap::new();
    let all_syms: Vec<&str> = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).copied().collect::<std::collections::HashSet<_>>().into_iter().collect();
    for sym in &all_syms {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let c = df.column("close")?.f64()?;
        let h = df.column("high")?.f64()?;
        let l = df.column("low")?.f64()?;
        let v = df.column("volume")?.f64()?;
        all_data.insert(sym.to_string(), SymData {
            close: c.into_iter().filter_map(|x| x).collect(),
            high:  h.into_iter().filter_map(|x| x).collect(),
            low:   l.into_iter().filter_map(|x| x).collect(),
            vol:   v.into_iter().filter_map(|x| x).collect(),
        });
    }

    let mut csv = String::from("chand_mult,universe,avg_return,avg_sharpe,avg_max_dd,avg_trades,avg_win_rate,pass_rate,windows_passed,total_windows\n");

    for &cm in M_VALUES {
        eprintln!("\n--- CHAND_MULT = {:.2} ---", cm);
        let mut global_sharpes = Vec::new();
        let mut global_pass = 0usize;
        let mut global_total = 0usize;

        for (uname, usyms) in UNIVERSES {
            let sd: HashMap<String, SymData> = usyms.iter().filter_map(|s| all_data.get(*s).map(|d| (s.to_string(), SymData { close: d.close.clone(), high: d.high.clone(), low: d.low.clone(), vol: d.vol.clone() }))).collect();
            let min_len = sd.values().map(|s| s.close.len()).min().unwrap_or(0);
            let nwin = (min_len.saturating_sub(TRAIN_BARS + TEST_BARS)) / TEST_BARS;
            let mut w_sharpes = Vec::new();
            let mut w_rets = Vec::new();
            let mut w_dds = Vec::new();
            let mut w_trades = Vec::new();
            let mut w_wrs = Vec::new();
            let mut passed = 0usize;

            for wi in 0..nwin {
                let te = TRAIN_BARS + wi * TEST_BARS;
                let tx = (te + TEST_BARS).min(min_len);
                let (t, r, s, dd, wr) = run_window(&sd, usyms, te, tx, cm);
                w_sharpes.push(s);
                w_rets.push(r);
                w_dds.push(dd);
                w_trades.push(t as f64);
                w_wrs.push(wr);
                if t >= MIN_TRADES && r > 0.0 { passed += 1; }
            }

            let avg_s: f64 = w_sharpes.iter().sum::<f64>() / w_sharpes.len().max(1) as f64;
            let avg_r: f64 = w_rets.iter().sum::<f64>() / w_rets.len().max(1) as f64;
            let avg_dd: f64 = w_dds.iter().sum::<f64>() / w_dds.len().max(1) as f64;
            let avg_t: f64 = w_trades.iter().sum::<f64>() / w_trades.len().max(1) as f64;
            let avg_wr: f64 = w_wrs.iter().sum::<f64>() / w_wrs.len().max(1) as f64;
            let pr = passed as f64 / nwin as f64 * 100.0;

            global_sharpes.extend(w_sharpes);
            global_pass += passed;
            global_total += nwin;

            eprintln!("  {:15}: Sharpe {:6.3} | Ret {:7.1}% | DD {:5.1}% | Trades {:4.0} | WR {:4.1}% | Pass {}/{} ({:.0}%)",
                uname, avg_s, avg_r*100.0, avg_dd*100.0, avg_t, avg_wr, passed, nwin, pr);

            csv.push_str(&format!("{:.2},{},{:.4},{:.3},{:.2},{:.1},{:.1},{:.0},{}/{}\n",
                cm, uname, avg_r, avg_s, avg_dd*100.0, avg_t, avg_wr, pr, passed, nwin));
        }

        let g_sharpe: f64 = global_sharpes.iter().sum::<f64>() / global_sharpes.len().max(1) as f64;
        let g_pr = global_pass as f64 / global_total as f64 * 100.0;
        eprintln!("  GLOBAL: Sharpe {:.3} | Pass {}/{} ({:.1}%)", g_sharpe, global_pass, global_total, g_pr);
    }

    std::fs::write("snapshots/chand_mult_9u_validation.csv", &csv)?;
    eprintln!("\nResults: snapshots/chand_mult_9u_validation.csv");
    Ok(())
}
