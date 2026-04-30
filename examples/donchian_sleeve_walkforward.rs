//! T31: Donchian as Portfolio Complement
//!
//! Tests Turtle(75%) + Donchian(25%) as a sleeve portfolio, not a replacement.
//! Both sleeves use the same production dual exit: Chandelier(7,2.30) OR Turtle ATR(24,2.0).
//! Walk-forward: Base5, 252 train / 252 test, realistic taker fees.

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
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;
const TURTLE_WEIGHT: f64 = 0.75;
const DONCHIAN_WEIGHT: f64 = 0.25;

const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const CSV_OUT: &str = "snapshots/donchian_sleeve_walkforward.csv";
const MD_OUT: &str = "snapshots/donchian_sleeve_walkforward.md";

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
    let mut total = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        total += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    total / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let breakout = close[idx] > max_close;
    if breakout && ATR_ENTRY_MULT > 0.0 {
        close[idx] >= max_close + ATR_ENTRY_MULT * atr_at(high, low, close, TURTLE_ATR_PERIOD, idx)
    } else {
        breakout
    }
}

fn donchian_signal(high: &[f64], close: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
    let max_high = high[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_high
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = if peak > 0.0 { (peak - e) / peak } else { 0.0 };
        max_dd = max_dd.max(dd);
    }
    max_dd * 100.0
}

fn metrics_from_daily(daily_rets: Vec<f64>, trades: usize, wins: usize) -> SimResult {
    let mut equity = 1.0;
    let mut curve = vec![1.0];
    for r in &daily_rets {
        equity *= 1.0 + r;
        curve.push(equity);
    }
    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&curve);
    let win_rate = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };
    let pass = trades >= MIN_TRADES && ret > 0.0;
    SimResult { ret, sharpe, max_dd, trades, win_rate, pass, daily_rets }
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
        let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        let mut entered = false;
        for sym in top_syms {
            let Some(sd) = sym_data.get(sym) else { continue; };
            if bar >= sd.close.len() || bar < TURTLE_ENTRY + 1 { continue; }
            let signal = if use_donchian {
                donchian_signal(&sd.high, &sd.close, bar)
            } else {
                turtle_signal(&sd.close, &sd.high, &sd.low, bar)
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
    // Wins are not directly meaningful after daily blending; keep trade count for thin-window checks.
    metrics_from_daily(daily_rets, turtle.trades + donchian.trades, 0)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== T31 Donchian Sleeve Walk-Forward ====");
    eprintln!("Portfolio: Turtle {:.0}% + Donchian {:.0}% | Base5 | 252/252 WF\n", TURTLE_WEIGHT*100.0, DONCHIAN_WEIGHT*100.0);

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in &SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = raw_cache.values().map(|df| df.height()).min().unwrap_or(0);
    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.to_string(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }

    let syms: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let n_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut csv = vec!["window,turtle_pass,turtle_sharpe,turtle_return,turtle_dd,turtle_trades,donchian_pass,donchian_sharpe,donchian_return,donchian_dd,donchian_trades,sleeve_pass,sleeve_sharpe,sleeve_return,sleeve_dd,sleeve_trades,delta_sharpe_vs_turtle,delta_return_vs_turtle".to_string()];
    let mut rows = Vec::new();

    let mut tur_pass = 0usize;
    let mut don_pass = 0usize;
    let mut slv_pass = 0usize;
    let mut tur_sh = 0.0;
    let mut don_sh = 0.0;
    let mut slv_sh = 0.0;
    let mut tur_ret = 0.0;
    let mut don_ret = 0.0;
    let mut slv_ret = 0.0;

    for wi in 0..n_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 30 { continue; }

        let turtle = run_strategy(&sym_data_map, &syms, test_start, test_end, false);
        let donchian = run_strategy(&sym_data_map, &syms, test_start, test_end, true);
        let sleeve = run_sleeve(&turtle, &donchian);

        tur_pass += turtle.pass as usize;
        don_pass += donchian.pass as usize;
        slv_pass += sleeve.pass as usize;
        tur_sh += turtle.sharpe;
        don_sh += donchian.sharpe;
        slv_sh += sleeve.sharpe;
        tur_ret += turtle.ret;
        don_ret += donchian.ret;
        slv_ret += sleeve.ret;

        eprintln!("W{:02} | Turtle {:>4} Sh {:+6.2} Ret {:+7.1}% | Don {:>4} Sh {:+6.2} Ret {:+7.1}% | Sleeve {:>4} Sh {:+6.2} Ret {:+7.1}% ΔSh {:+6.2}",
            wi,
            if turtle.pass {"PASS"} else {"FAIL"}, turtle.sharpe, turtle.ret,
            if donchian.pass {"PASS"} else {"FAIL"}, donchian.sharpe, donchian.ret,
            if sleeve.pass {"PASS"} else {"FAIL"}, sleeve.sharpe, sleeve.ret,
            sleeve.sharpe - turtle.sharpe,
        );

        csv.push(format!("{},{},{:.6},{:.4},{:.4},{},{},{:.6},{:.4},{:.4},{},{},{:.6},{:.4},{:.4},{},{:.6},{:.4}",
            wi,
            turtle.pass, turtle.sharpe, turtle.ret, turtle.max_dd, turtle.trades,
            donchian.pass, donchian.sharpe, donchian.ret, donchian.max_dd, donchian.trades,
            sleeve.pass, sleeve.sharpe, sleeve.ret, sleeve.max_dd, sleeve.trades,
            sleeve.sharpe - turtle.sharpe, sleeve.ret - turtle.ret,
        ));
        rows.push((wi, turtle, donchian, sleeve));
    }

    let n_f = rows.len().max(1) as f64;
    let turtle_pass_rate = tur_pass as f64 / n_f * 100.0;
    let sleeve_pass_rate = slv_pass as f64 / n_f * 100.0;
    let turtle_avg_sharpe = tur_sh / n_f;
    let sleeve_avg_sharpe = slv_sh / n_f;
    let sharpe_delta_pct = if turtle_avg_sharpe.abs() > 1e-9 { (sleeve_avg_sharpe - turtle_avg_sharpe) / turtle_avg_sharpe * 100.0 } else { 0.0 };
    let pass_delta_pp = sleeve_pass_rate - turtle_pass_rate;
    let reject = sharpe_delta_pct < -10.0 || pass_delta_pp < -5.0 || sleeve_avg_sharpe < turtle_avg_sharpe;

    let mut f = File::create(CSV_OUT)?;
    for line in &csv { writeln!(f, "{}", line)?; }

    let mut md = String::new();
    md.push_str("# T31 Donchian Sleeve Walk-Forward\n\n");
    md.push_str("**Hypothesis:** Donchian entry is too sparse as a replacement, but may diversify Turtle as a 25% high-conviction sleeve.\n\n");
    md.push_str("**Method:** Base5, 252/252 walk-forward, Turtle(75%) + Donchian(25%), same production dual exit and taker fees.\n\n");
    md.push_str("| Metric | Turtle | Donchian | 75/25 Sleeve | Sleeve vs Turtle |\n");
    md.push_str("|---|---:|---:|---:|---:|\n");
    md.push_str(&format!("| Pass Rate | {}/{} ({:.0}%) | {}/{} ({:.0}%) | {}/{} ({:.0}%) | {:+.1} pp |\n",
        tur_pass, rows.len(), turtle_pass_rate,
        don_pass, rows.len(), don_pass as f64 / n_f * 100.0,
        slv_pass, rows.len(), sleeve_pass_rate,
        sleeve_pass_rate - turtle_pass_rate));
    md.push_str(&format!("| Avg Sharpe | {:+.3} | {:+.3} | {:+.3} | {:+.3} |\n", turtle_avg_sharpe, don_sh/n_f, sleeve_avg_sharpe, sleeve_avg_sharpe - turtle_avg_sharpe));
    md.push_str(&format!("| Avg Return | {:+.1}% | {:+.1}% | {:+.1}% | {:+.1}% |\n", tur_ret/n_f, don_ret/n_f, slv_ret/n_f, slv_ret/n_f - tur_ret/n_f));
    md.push_str("\n## Decision\n\n");
    if reject {
        md.push_str(&format!("**REJECTED.** Sleeve fails the T31 promotion rule: Sharpe delta {:+.1}% and pass-rate delta {:+.1} pp. Turtle-only remains production default.\n\n", sharpe_delta_pct, pass_delta_pp));
    } else {
        md.push_str(&format!("**CANDIDATE.** Sleeve passes the T31 guardrail: Sharpe delta {:+.1}% and pass-rate delta {:+.1} pp. Needs broader 9-universe validation before promotion.\n\n", sharpe_delta_pct, pass_delta_pp));
    }
    md.push_str("## Per-Window Results\n\n");
    md.push_str("| Window | Turtle | Turtle Sh | Turtle Ret | Donchian | Don Sh | Don Ret | Sleeve | Sleeve Sh | Sleeve Ret | ΔSh |\n");
    md.push_str("|---|---|---:|---:|---|---:|---:|---|---:|---:|---:|\n");
    for (wi, turtle, donchian, sleeve) in &rows {
        md.push_str(&format!("| W{:02} | {} | {:+.2} | {:+.1}% | {} | {:+.2} | {:+.1}% | {} | {:+.2} | {:+.1}% | {:+.2} |\n",
            wi,
            if turtle.pass {"✅"} else {"❌"}, turtle.sharpe, turtle.ret,
            if donchian.pass {"✅"} else {"❌"}, donchian.sharpe, donchian.ret,
            if sleeve.pass {"✅"} else {"❌"}, sleeve.sharpe, sleeve.ret,
            sleeve.sharpe - turtle.sharpe));
    }
    let mut f = File::create(MD_OUT)?;
    f.write_all(md.as_bytes())?;

    eprintln!("\n=== SUMMARY ===");
    eprintln!("Turtle:  {}/{} pass | avg Sharpe {:+.3} | avg Ret {:+.1}%", tur_pass, rows.len(), turtle_avg_sharpe, tur_ret/n_f);
    eprintln!("Donchian:{}/{} pass | avg Sharpe {:+.3} | avg Ret {:+.1}%", don_pass, rows.len(), don_sh/n_f, don_ret/n_f);
    eprintln!("Sleeve:  {}/{} pass | avg Sharpe {:+.3} | avg Ret {:+.1}%", slv_pass, rows.len(), sleeve_avg_sharpe, slv_ret/n_f);
    eprintln!("Decision: {}", if reject {"REJECT"} else {"CANDIDATE"});
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("MD: {}", MD_OUT);
    eprintln!("Runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
