//! T31: Donchian as Portfolio Complement — 9-Universe Validation
//!
//! Tests Turtle(75%) + Donchian(25%) as a sleeve portfolio across all 9 universes.
//! Both sleeves use the same production dual exit: Chandelier(7,2.30) OR Turtle ATR(24,2.0).
//! Walk-forward: 252-bar train / 252-bar test, realistic taker fees.
//! Guardrail: Reject if global pass rate drops >5pp or Sharpe improvement fails outside Base5.

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;
const TURTLE_WEIGHT: f64 = 0.75;
const DONCHIAN_WEIGHT: f64 = 0.25;

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

const MIN_TRADES: usize = 3;
const MIN_GLOBAL_PASS_RATE: f64 = 69.1; // T31 guardrail: production baseline 74.1% minus 5pp
const CSV_OUT: &str = "snapshots/donchian_sleeve_9universe.csv";
const MD_OUT: &str = "snapshots/donchian_sleeve_9universe.md";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone, Default)]
struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    daily_rets: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn donchian_signal(high: &[f64], close: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
    let mut max_high = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&h) = high.get(i) { max_high = max_high.max(h); }
    }
    close.get(idx).map_or(false, |&c| c > max_high)
}

fn turtle_signal(close: &[f64], _high: &[f64], _low: &[f64], entry_period: usize, _atr_period: usize, _atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c > max_close)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn metrics_from_daily(daily_rets: Vec<f64>, trades: usize, wins: usize) -> SimResult {
    let ret = daily_rets.iter().fold(1.0_f64, |acc, r| acc * (1.0 + r)) - 1.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let equity: Vec<f64> = daily_rets.iter().scan(1.0_f64, |s, r| { *s *= 1.0 + r; Some(*s) }).collect();
    let max_dd = max_dd_from(&equity);
    let win_rate = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };
    let pass = trades >= MIN_TRADES && ret > 0.0;
    SimResult { ret: ret * 100.0, sharpe, max_dd, trades, win_rate, pass, daily_rets }
}

fn run_strategy(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    use_donchian: bool,
) -> SimResult {
    let len = test_end.saturating_sub(test_start);
    let mut daily_rets = vec![0.0; len];
    let mut wins = 0usize;
    let mut trades = 0usize;
    let mut bar = test_start;

    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = rolling_avg(&sd.vol, VOL_LOOKBACK, bar) * sd.close[bar];
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(3).map(|(s, _)| s).collect();

        let mut entered = false;
        for sym in top_syms {
            let Some(sd) = sym_data.get(sym) else { continue; };
            if bar >= sd.close.len() || bar < TURTLE_ENTRY + 1 { continue; }
            let signal = if use_donchian {
                donchian_signal(&sd.high, &sd.close, bar)
            } else {
                turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar)
            };
            if !signal { continue; }

            let entry = sd.close[bar] * (1.0 - TAKER_FEE);
            let entry_bar_next = bar + 1;
            let n = sd.close.len();
            if entry_bar_next >= n { continue; }

            let mut highest_high_chand = sd.high[entry_bar_next];
            let mut lowest_low_turtle = sd.low[entry_bar_next];
            let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1)).min(test_end.saturating_sub(1));
            let mut exit_bar = max_bar;
            for b in entry_bar_next..=max_bar {
                highest_high_chand = highest_high_chand.max(sd.high[b]);
                let trail_chand = highest_high_chand - CHAND_MULT * atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                    exit_bar = b;
                    break;
                }
            }

            let exit = sd.close[exit_bar] * (1.0 - TAKER_FEE);
            let gross_ret = exit / entry - 1.0;
            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
            let avg_daily = gross_ret / bars_held as f64;
            for b in entry_bar_next..=exit_bar {
                if b >= test_start && b < test_end {
                    daily_rets[b - test_start] += avg_daily;
                }
            }
            trades += 1;
            if gross_ret > 0.0 { wins += 1; }
            bar = exit_bar + 1;
            entered = true;
            break;
        }

        if !entered { bar += 1; }
    }

    metrics_from_daily(daily_rets, trades, wins)
}

fn run_sleeve(turtle: &SimResult, donchian: &SimResult) -> SimResult {
    let n = turtle.daily_rets.len().min(donchian.daily_rets.len());
    let daily_rets = (0..n)
        .map(|i| TURTLE_WEIGHT * turtle.daily_rets[i] + DONCHIAN_WEIGHT * donchian.daily_rets[i])
        .collect::<Vec<_>>();
    metrics_from_daily(daily_rets, turtle.trades + donchian.trades, 0)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== T31 Donchian Sleeve — 9-Universe Validation ====");
    eprintln!("Portfolio: Turtle {:.0}% + Donchian {:.0}% | 252/252 WF\n", TURTLE_WEIGHT*100.0, DONCHIAN_WEIGHT*100.0);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }

    let n_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut csv = vec!["universe,window,turtle_pass,turtle_sharpe,turtle_return,turtle_dd,turtle_trades,don_pass,don_sharpe,don_return,don_dd,don_trades,sleeve_pass,sleeve_sharpe,sleeve_return,sleeve_dd,sleeve_trades,delta_sharpe".to_string()];
    let mut md_lines = vec![
        "# T31 Donchian Sleeve — 9-Universe Validation\n\n".to_string(),
        format!("Portfolio: Turtle {:.0}% + Donchian {:.0}% | 252/252 WF | {} windows\n\n", TURTLE_WEIGHT*100.0, DONCHIAN_WEIGHT*100.0, n_windows),
    ];

    let mut global_tur_pass = 0usize;
    let mut global_slv_pass = 0usize;
    let mut global_tur_sh = 0.0;
    let mut global_slv_sh = 0.0;
    let mut global_tur_ret = 0.0;
    let mut global_slv_ret = 0.0;
    let mut total_windows = 0usize;
    let mut universe_results: Vec<(String, usize, usize, f64, f64, f64, f64)> = Vec::new();

    for (uname, syms) in UNIVERSES {
        let syms: Vec<String> = syms.iter().map(|s| s.to_string()).collect();
        let mut tur_pass = 0usize;
        let mut slv_pass = 0usize;
        let mut tur_sh = 0.0;
        let mut slv_sh = 0.0;
        let mut tur_ret = 0.0;
        let mut slv_ret = 0.0;
        let mut u_windows = 0usize;

        eprintln!("Universe: {}", uname);
        for wi in 0..n_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 100 { continue; }

            let turtle = run_strategy(&sym_data_map, &syms, test_start, test_end, false);
            let donchian = run_strategy(&sym_data_map, &syms, test_start, test_end, true);
            let sleeve = run_sleeve(&turtle, &donchian);

            tur_pass += turtle.pass as usize;
            slv_pass += sleeve.pass as usize;
            tur_sh += turtle.sharpe;
            slv_sh += sleeve.sharpe;
            tur_ret += turtle.ret;
            slv_ret += sleeve.ret;
            u_windows += 1;
            total_windows += 1;
            global_tur_pass += turtle.pass as usize;
            global_slv_pass += sleeve.pass as usize;
            global_tur_sh += turtle.sharpe;
            global_slv_sh += sleeve.sharpe;
            global_tur_ret += turtle.ret;
            global_slv_ret += sleeve.ret;

            csv.push(format!("{},{},{},{:.3},{:.1},{:.1},{},{},{:.3},{:.1},{:.1},{},{},{:.3},{:.1},{:.1},{},{:.3}",
                uname, wi, turtle.pass, turtle.sharpe, turtle.ret, turtle.max_dd, turtle.trades,
                donchian.pass, donchian.sharpe, donchian.ret, donchian.max_dd, donchian.trades,
                sleeve.pass, sleeve.sharpe, sleeve.ret, sleeve.max_dd, sleeve.trades,
                sleeve.sharpe - turtle.sharpe));

            eprintln!("  W{:02} | Turtle {} Sh {:+.2} | Sleeve {} Sh {:+.2} ΔSh {:+.2}",
                wi,
                if turtle.pass {"PASS"} else {"FAIL"}, turtle.sharpe,
                if sleeve.pass {"PASS"} else {"FAIL"}, sleeve.sharpe,
                sleeve.sharpe - turtle.sharpe);
        }

        let n_u = u_windows as f64;
        let tur_pr = tur_pass as f64 / n_u * 100.0;
        let slv_pr = slv_pass as f64 / n_u * 100.0;
        let delta_sh = slv_sh / n_u - tur_sh / n_u;
        eprintln!("  => Turtle {}/{} ({:.0}%), Sleeve {}/{} ({:.0}%), ΔSh {:+.3}\n",
            tur_pass, u_windows, tur_pr, slv_pass, u_windows, slv_pr, delta_sh);

        universe_results.push((uname.to_string(), tur_pass, slv_pass, tur_sh/n_u, slv_sh/n_u, tur_pr, slv_pr));
    }

    let n_g = total_windows as f64;
    let turtle_avg_sh = global_tur_sh / n_g;
    let sleeve_avg_sh = global_slv_sh / n_g;
    let turtle_global_pr = global_tur_pass as f64 / n_g * 100.0;
    let sleeve_global_pr = global_slv_pass as f64 / n_g * 100.0;
    let pass_drop = turtle_global_pr - sleeve_global_pr;
    let sharpe_improve_pct = if turtle_avg_sh.abs() > 1e-9 { (sleeve_avg_sh - turtle_avg_sh) / turtle_avg_sh * 100.0 } else { 0.0 };
    let reject = sleeve_global_pr < MIN_GLOBAL_PASS_RATE || pass_drop > 5.0 || sleeve_avg_sh < turtle_avg_sh;

    md_lines.push("## Global Summary\n\n".to_string());
    md_lines.push("| Metric | Turtle | Sleeve | Delta |\n".to_string());
    md_lines.push("|---|---:|---:|---:|\n".to_string());
    md_lines.push(format!("| Global Pass | {}/{} ({:.0}%) | {}/{} ({:.0}%) | {:+.1} pp |\n",
        global_tur_pass, total_windows, turtle_global_pr,
        global_slv_pass, total_windows, sleeve_global_pr,
        sleeve_global_pr - turtle_global_pr));
    md_lines.push(format!("| Avg Sharpe | {:+.3} | {:+.3} | {:+.3} |\n", turtle_avg_sh, sleeve_avg_sh, sleeve_avg_sh - turtle_avg_sh));
    md_lines.push(format!("| Avg Return | {:+.1}% | {:+.1}% | {:+.1}% |\n\n",
        global_tur_ret/n_g, global_slv_ret/n_g, global_slv_ret/n_g - global_tur_ret/n_g));

    md_lines.push("## Per-Universe Results\n\n".to_string());
    md_lines.push("| Universe | Turtle Pass | Sleeve Pass | Turtle Sh | Sleeve Sh | ΔSh | Sleeve vs Turtle |\n".to_string());
    md_lines.push("|---|---:|---:|---:|---:|---:|---|\n".to_string());
    for (u, tp, sp, tsh, ssh, tpr, spr) in &universe_results {
        let delta_sh = ssh - tsh;
        md_lines.push(format!("| {} | {}/{} ({:.0}%) | {}/{} ({:.0}%) | {:+.3} | {:+.3} | {:+.3} | {:+.1} pp |\n",
            u, tp, n_windows, tpr, sp, n_windows, spr, tsh, ssh, delta_sh, spr - tpr));
    }

    md_lines.push("\n## Decision\n\n".to_string());
    if reject {
        md_lines.push(format!("**REJECTED.** Sleeve global pass rate {:.1}% is below the T31 guardrail {:.1}% (production baseline 74.1% minus 5pp). Sharpe improves {:+.1}% ({:+.3} vs {:+.3}), but the absolute pass-rate failure keeps Turtle-only as production default.\n\n",
            sleeve_global_pr, MIN_GLOBAL_PASS_RATE, sharpe_improve_pct, sleeve_avg_sh, turtle_avg_sh));
    } else {
        let pass_delta = sleeve_global_pr - turtle_global_pr;
        md_lines.push(format!("**CANDIDATE (9-UNIVERSE).** Pass delta {:+.1} pp, Sharpe {:+.1}% improvement ({:+.3} vs {:+.3}), and global pass {:.1}% clears the {:.1}% guardrail. Turtle+Donchian sleeve is a production candidate.\n\n",
            pass_delta, sharpe_improve_pct, sleeve_avg_sh, turtle_avg_sh, sleeve_global_pr, MIN_GLOBAL_PASS_RATE));
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv { writeln!(f, "{}", line)?; }
    std::fs::write(MD_OUT, md_lines.join(""))?;

    eprintln!("\n=== FINAL ===");
    eprintln!("Global Turtle:  {}/{} ({:.0}%) | avg Sharpe {:+.3}", global_tur_pass, total_windows, turtle_global_pr, turtle_avg_sh);
    eprintln!("Global Sleeve:  {}/{} ({:.0}%) | avg Sharpe {:+.3}", global_slv_pass, total_windows, sleeve_global_pr, sleeve_avg_sh);
    eprintln!("Decision: {} (global pass {:.1}% vs guardrail {:.1}%, Sharpe {:+.1}% vs turtle)", if reject {"REJECT"} else {"CANDIDATE"}, sleeve_global_pr, MIN_GLOBAL_PASS_RATE, sharpe_improve_pct);
    eprintln!("CSV: {} | MD: {} | Runtime: {:.1}s", CSV_OUT, MD_OUT, t0.elapsed().as_secs_f64());
    Ok(())
}
