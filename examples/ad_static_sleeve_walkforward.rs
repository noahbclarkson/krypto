//! A/D Static Sleeve Walk-Forward — Turtle(80%) + A/D period=8 (20%)
//!
//! Test: does adding a 20% A/D sleeve improve Turtle-only Sharpe?
//!
//! 4bp taker fee each side. Walk-forward: 252-bar train, 126-bar test.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

// Frozen Turtle+Chandelier params
const TURTLE_EP: usize = 21;
const TURTLE_ATR_P: usize = 25;
const TURTLE_ATR_M: f64 = 2.0;
const CHAND_P_TURTLE: usize = 28;
const CHAND_M_TURTLE: f64 = 2.0;
const TURTLE_HOLD_MAX: usize = 45;
const POSITION_CAP: usize = 3;

// A/D params (walk-forward winner)
const AD_PERIOD: usize = 8;
const CHAND_P_AD: usize = 15;
const CHAND_M_AD: f64 = 2.0;
const AD_HOLD_MAX: usize = 60;

const TAKER_FEE: f64 = 0.0004;
const CANDLES: u32 = 3000;
const WF_TRAIN: usize = 252;
const WF_TEST: usize = 126;
const MIN_TRAIN: usize = 252;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LowVolume5",   &["BTCUSDT","LTCUSDT","EOSUSDT","BNBUSDT","BCHUSDT"]),
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = *high.get(i).unwrap_or(&0.0);
        let l = *low.get(i).unwrap_or(&0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn max_close(close: &[f64], start: usize, end: usize) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for i in start..end { // exclusive — matches turtle_chandelier_walkforward.rs
        if let Some(&c) = close.get(i) { m = m.max(c); }
    }
    m
}

struct TurtlePos {
    sym: String,
    entry: f64,
    hh_c: f64,
    hh_t: f64,
    hold: usize,
}

struct AdPos {
    sym: String,
    entry: f64,
    hh: f64,
    hold: usize,
}

fn fmt_f(v: f64, prec: usize) -> String {
    // Use scientific notation (Rust 1.94 removed fixed-point 'f' format trait)
    format!("{:.prec$e}", v, prec = prec)
}
fn fmt_f2(v: f64) -> String { fmt_f(v, 2) }
fn fmt_f3(v: f64) -> String { fmt_f(v, 3) }
fn fmt_f4(v: f64) -> String { fmt_f(v, 4) }

fn run_window(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64) {
    let warmup = CHAND_P_TURTLE.max(TURTLE_ATR_P).max(AD_PERIOD * 4) + TURTLE_ATR_P;
    let n = sym_data.get(symbols[0]).map(|d| d.close.len()).unwrap_or(0);

    let mut turtle_daily = Vec::new();
    let mut ad_daily = Vec::new();
    let mut turtle_equity = 1.0_f64;
    let mut ad_equity = 1.0_f64;
    let mut turtle_pos: Vec<TurtlePos> = Vec::new();
    let mut ad_pos: Vec<AdPos> = Vec::new();

    for bar in warmup..n {
        let in_test = bar >= test_start && bar < test_end;
        if !in_test {
            turtle_daily.push(turtle_equity);
            ad_daily.push(ad_equity);
            continue;
        }

        // Turtle exits
        let mut kept = Vec::new();
        for pos in turtle_pos.drain(..) {
            let sd = match sym_data.get(&pos.sym) {
                Some(s) => s,
                None => { kept.push(pos); continue; }
            };
            if bar >= sd.close.len() { kept.push(pos); continue; }
            let curr = *sd.close.get(bar).unwrap_or(&pos.entry);
            let atr_c = atr(&sd.high, &sd.low, &sd.close, CHAND_P_TURTLE, bar);
            let atr_t = atr(&sd.high, &sd.low, &sd.close, TURTLE_ATR_P, bar);
            let trail_c = pos.hh_c - CHAND_M_TURTLE * atr_c;
            let trail_t = pos.hh_t - TURTLE_ATR_M * atr_t;
            let exited = curr < trail_c || curr < trail_t || pos.hold >= TURTLE_HOLD_MAX;
            if exited {
                let exit_px = curr * (1.0 - TAKER_FEE);
                turtle_equity *= exit_px / pos.entry;
            } else {
                let hh_c = pos.hh_c.max(*sd.high.get(bar).unwrap_or(&pos.entry));
                let hh_t = pos.hh_t.max(*sd.high.get(bar).unwrap_or(&pos.entry));
                kept.push(TurtlePos { sym: pos.sym, entry: pos.entry, hh_c, hh_t, hold: pos.hold + 1 });
            }
        }
        turtle_pos = kept;

        // A/D exits
        let mut kept_ad = Vec::new();
        for pos in ad_pos.drain(..) {
            let sd = match sym_data.get(&pos.sym) {
                Some(s) => s,
                None => { kept_ad.push(pos); continue; }
            };
            if bar >= sd.close.len() { kept_ad.push(pos); continue; }
            let curr = *sd.close.get(bar).unwrap_or(&pos.entry);
            let atr_a = atr(&sd.high, &sd.low, &sd.close, CHAND_P_AD, bar);
            let trail = pos.hh - CHAND_M_AD * atr_a;
            let exited = curr < trail || pos.hold >= AD_HOLD_MAX;
            if exited {
                let exit_px = curr * (1.0 - TAKER_FEE);
                ad_equity *= exit_px / pos.entry;
            } else {
                let hh = pos.hh.max(*sd.high.get(bar).unwrap_or(&pos.entry));
                kept_ad.push(AdPos { sym: pos.sym, entry: pos.entry, hh, hold: pos.hold + 1 });
            }
        }
        ad_pos = kept_ad;

        // Turtle entries
        if turtle_pos.len() < POSITION_CAP {
            let mut scores: Vec<(&str, f64)> = symbols.iter()
                .filter_map(|&s| {
                    let sd = sym_data.get(s)?;
                    if bar > 0 && (bar - 1) < sd.close.len() && (bar - 1) < sd.vol.len() {
                        let dv = sd.vol.get(bar - 1)? * sd.close.get(bar - 1)?;
                        Some((s, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }))
                    } else { None }
                })
                .collect();
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            for (sym_str, _) in scores.into_iter().take(POSITION_CAP) {
                if turtle_pos.iter().any(|p| &p.sym == sym_str) { continue; }
                if turtle_pos.len() >= POSITION_CAP { break; }
                let sd = match sym_data.get(sym_str) {
                    Some(s) => s,
                    None => continue,
                };
                let prev = bar - 1;
                if prev < TURTLE_EP + 1 || prev >= sd.close.len() || bar >= sd.close.len() { continue; }
                let start_idx = prev + 1 - TURTLE_EP;
                let max_c = max_close(&sd.close, start_idx, prev);
                let prev_close = *sd.close.get(prev).unwrap_or(&0.0);
                if prev_close <= max_c { continue; }
                let entry_px = *sd.close.get(bar).unwrap_or(&prev_close) * (1.0 + TAKER_FEE);
                let hh_0 = *sd.high.get(bar).unwrap_or(&entry_px);
                turtle_pos.push(TurtlePos { sym: sym_str.to_string(), entry: entry_px, hh_c: hh_0, hh_t: hh_0, hold: 0 });
            }
        }

        // A/D entries (momentum long only)
        if ad_pos.len() < 2 {
            let mut ad_scores: Vec<(&str, f64)> = Vec::new();
            for &sym_str in symbols {
                let sd = match sym_data.get(sym_str) {
                    Some(s) => s,
                    None => continue,
                };
                if bar < AD_PERIOD * 4 || bar >= sd.close.len() { continue; }
                let curr_ad = if let (Some(&c_curr), Some(&c_prev)) = (
                    sd.close.get(bar),
                    sd.close.get(bar.saturating_sub(AD_PERIOD)),
                ) { c_curr / c_prev - 1.0 } else { 0.0 };
                ad_scores.push((sym_str, curr_ad));
            }
            ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            for (sym_str, mom) in ad_scores.into_iter().take(2) {
                if ad_pos.iter().any(|p| &p.sym == sym_str) { continue; }
                if ad_pos.len() >= 2 { break; }
                if mom <= 0.0 { continue; }
                let sd = match sym_data.get(sym_str) {
                    Some(s) => s,
                    None => continue,
                };
                if bar >= sd.close.len() { continue; }
                let entry_px = *sd.close.get(bar).unwrap_or(&0.0) * (1.0 + TAKER_FEE);
                let hh_0 = *sd.high.get(bar).unwrap_or(&entry_px);
                ad_pos.push(AdPos { sym: sym_str.to_string(), entry: entry_px, hh: hh_0, hold: 0 });
            }
        }

        turtle_daily.push(turtle_equity);
        ad_daily.push(ad_equity);
    }

    let turtle_sharpe = compute_sharpe(&turtle_daily);
    let ad_sharpe = compute_sharpe(&ad_daily);
    let sleeve_sharpe = compute_composite_sharpe(&turtle_daily, &ad_daily, 0.80, 0.20);
    (turtle_sharpe, ad_sharpe, sleeve_sharpe)
}

fn compute_sharpe(daily: &[f64]) -> f64 {
    let mut rets = Vec::new();
    for i in 1..daily.len() {
        if daily[i-1] > 0.0 {
            rets.push(daily[i] / daily[i-1] - 1.0);
        }
    }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    let std = var.sqrt();
    if std < 1e-10 { return 0.0; }
    mean / std * (252.0_f64).sqrt()
}

fn compute_composite_sharpe(turtle: &[f64], ad: &[f64], t_frac: f64, a_frac: f64) -> f64 {
    let mut blended_rets = Vec::new();
    let min_len = turtle.len().min(ad.len());
    for i in 1..min_len {
        if turtle[i-1] > 0.0 && ad[i-1] > 0.0 {
            let t_ret = turtle[i] / turtle[i-1] - 1.0;
            let a_ret = ad[i] / ad[i-1] - 1.0;
            blended_rets.push(t_frac * t_ret + a_frac * a_ret);
        }
    }
    if blended_rets.is_empty() { return 0.0; }
    let mean = blended_rets.iter().sum::<f64>() / blended_rets.len() as f64;
    let var = blended_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / blended_rets.len() as f64;
    let std = var.sqrt();
    if std < 1e-10 { return 0.0; }
    mean / std * (252.0_f64).sqrt()
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("\n==== A/D Static Sleeve Walk-Forward ====");
    eprintln!("Test: Turtle(80%) + A/D period={}(20%) vs Turtle-only\n", AD_PERIOD);

    let loader = DataLoader::new(None, None);
    let mut all_turtle = Vec::new();
    let mut all_sleeve = Vec::new();
    let mut passed = 0usize;
    let mut total = 0usize;
    let mut csv_lines = vec!["universe,win,turtle_sharpe,ad_sharpe,sleeve_sharpe,improvement_pct,winner".to_string()];

    for &(uni_name, symbols) in UNIVERSES {
        let mut sym_data: HashMap<String, SymData> = HashMap::new();
        for &sym in symbols {
            match loader.fetch_with_cache(sym, "1d", CANDLES).await {
                Ok(df) => {
                    sym_data.insert(sym.to_string(), SymData {
                        close: df.column("close")?.f64()?.into_iter().filter_map(|x| x).collect(),
                        high:  df.column("high")?.f64()?.into_iter().filter_map(|x| x).collect(),
                        low:   df.column("low")?.f64()?.into_iter().filter_map(|x| x).collect(),
                        vol:   df.column("volume")?.f64()?.into_iter().filter_map(|x| x).collect(),
                    });
                }
                Err(e) => { eprintln!("  ERROR loading {}: {}", sym, e); }
            }
        }
        let n = sym_data.get(symbols[0]).map(|d| d.close.len()).unwrap_or(0);
        eprintln!("{}: {} bars, {} symbols", uni_name, n, symbols.len());

        let mut window_start = MIN_TRAIN;
        let mut window_idx = 0usize;
        while window_start + WF_TEST <= n {
            let test_start = window_start;
            let test_end = (test_start + WF_TEST).min(n);
            let (t_sharpe, ad_sharpe, sleeve_sharpe) =
                run_window(&sym_data, symbols, test_start, test_end);

            let improve = if t_sharpe.abs() > 0.01 {
                (sleeve_sharpe - t_sharpe) / t_sharpe.abs() * 100.0
            } else { 0.0 };
            let winner = if sleeve_sharpe > t_sharpe { "SLEEVE" } else { "TURTLE" };
            if sleeve_sharpe > t_sharpe { passed += 1; }
            total += 1;

            all_turtle.push(t_sharpe);
            all_sleeve.push(sleeve_sharpe);

            csv_lines.push(format!("{},{},{},{},{},{},{}",
                uni_name, window_idx, fmt_f4(t_sharpe), fmt_f4(ad_sharpe), fmt_f4(sleeve_sharpe), fmt_f2(improve), winner));
            eprintln!("  W{}: turtle={} ad={} sleeve={} imp={}% -> {}",
                window_idx, fmt_f3(t_sharpe), fmt_f3(ad_sharpe), fmt_f3(sleeve_sharpe), fmt_f2(improve), winner);

            window_start += WF_TEST;
            window_idx += 1;
        }
    }

    let avg_t = all_turtle.iter().sum::<f64>() / all_turtle.len().max(1) as f64;
    let avg_s = all_sleeve.iter().sum::<f64>() / all_sleeve.len().max(1) as f64;
    let improve_total = if avg_t.abs() > 0.01 { (avg_s - avg_t) / avg_t.abs() * 100.0 } else { 0.0 };

    eprintln!("\n==== SUMMARY ====");
    eprintln!("Sleeve beats Turtle: {}/{} ({:.0}%)\n", passed, total, passed as f64 / total.max(1) as f64 * 100.0);
    eprintln!("Avg Sharpe Turtle-only:  {}", fmt_f4(avg_t));
    eprintln!("Avg Sharpe 80/20 Sleeve: {}", fmt_f4(avg_s));
    eprintln!("Avg Improvement: {}%", fmt_f2(improve_total));

    let verdict = if passed as f64 / total.max(1) as f64 > 0.55 {
        "A/D SLEEVE IMPROVES -- add to production params"
    } else {
        "A/D SLEEVE DOES NOT IMPROVE -- Turtle-only is production"
    };
    eprintln!("\nCONCLUSION: {}", verdict);

    let csv_path = "snapshots/ad_static_sleeve_results.csv";
    let mut f = std::fs::File::create(csv_path)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nCSV: {}", csv_path);
    eprintln!("Runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
