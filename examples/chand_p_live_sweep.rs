//! CHAND_PERIOD Hyperopt — Turtle-Only Live Path
//!
//! Sweep CHAND_PERIOD P ∈ [5..=60 step 1] under the Turtle-only live path.
//! Prior dual-exit sweep found P=7. This tests P on the actual live Turtle-only path.
//! Produces: snapshots/chand_p_live_sweep.csv (detailed), snapshots/chand_p_live_summary.csv,
//! snapshots/chand_p_live_selected_equity.csv, and charts/comparison_chart.png.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 8;

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0;

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

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_close
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

#[derive(Default)]
struct WindowResult {
    pass: i32,
    sharpe: f64,
    ret_pct: f64,
    dd_pct: f64,
    trades: i32,
}

#[derive(Default)]
struct SummaryResult {
    pass_count: i32,
    total_count: i32,
    avg_sharpe: f64,
    avg_ret: f64,
    avg_dd: f64,
    total_trades: i32,
    positive_universes: i32,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

        if btc_pct < ATR_RANK_T {
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::with_capacity(252);
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult = 0.70;
                                }
                            }
                        }

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();

                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }

                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * size_mult;

                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered {
            bar += 1;
        }
    }

    (equity, annualised_sharpe(&daily_rets), max_dd_from(&[peak]), total_trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { return Ok(()); }
    println!("{} windows per universe, {} universes", windows, UNIVERSES.len());

    // CHAND_PERIOD sweep: 5..=60 step 1
    let sweep_values: Vec<usize> = (5..=60).step_by(1).collect();
    let n_vals = sweep_values.len();
    println!("Sweeping P ∈ [5..=60] step 1 → {} values × 9 universes × {} windows = {} runs",
        n_vals, windows, n_vals * 9 * windows);

    // Per-value aggregates
    let mut summary: HashMap<usize, SummaryResult> = HashMap::new();
    let mut equity_aggregates: HashMap<usize, Vec<f64>> = HashMap::new();
    for &p in &sweep_values {
        summary.insert(p, SummaryResult::default());
        equity_aggregates.insert(p, vec![1.0_f64; windows]);
    }

    // Per-universe per-window results for summary
    let mut univ_pass_count: HashMap<&str, i32> = HashMap::new();
    let mut univ_total_count: HashMap<&str, i32> = HashMap::new();

    let start_time = std::time::Instant::now();
    for (u_idx, (u_name, u_syms)) in UNIVERSES.iter().enumerate() {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;

            for &p in &sweep_values {
                let (eq, sharpe, dd, trades) = run_sim(&sym_data, &syms, start + TRAIN_BARS, end);

                let pass = if trades >= MIN_TRADES && sharpe > 0.0 { 1 } else { 0 };
                let ret_pct = (eq - 1.0) * 100.0;

                // Aggregate
                if let Some(s) = summary.get_mut(&p) {
                    s.pass_count += pass;
                    s.total_count += 1;
                    s.avg_sharpe += sharpe;
                    s.avg_ret += ret_pct;
                    s.avg_dd += dd;
                    s.total_trades += trades as i32;
                }

                // Per-universe counts
                *univ_pass_count.entry(u_name).or_insert(0) += pass;
                *univ_total_count.entry(u_name).or_insert(0) += 1;

                // Equity aggregate (Base5 only — window-compounded)
                if u_name == &"Base5" {
                    if let Some(agg) = equity_aggregates.get_mut(&p) {
                        if w < agg.len() {
                            agg[w] *= eq;
                        }
                    }
                }
            }
        }
        if (u_idx + 1) % 3 == 0 {
            let elapsed = start_time.elapsed().as_secs_f64();
            let estimated = elapsed / (u_idx + 1) as f64 * UNIVERSES.len() as f64;
            println!("  Universe {}/{} done, elapsed {:.1}s, est. total {:.1}s", u_idx+1, UNIVERSES.len(), elapsed, estimated);
        }
    }

    // Finalize averages
    for (&_p, s) in summary.iter_mut() {
        let n = s.total_count as f64;
        s.avg_sharpe /= n;
        s.avg_ret /= n;
        s.avg_dd /= n;
        s.positive_universes = univ_pass_count.iter().filter(|(u, &pc)| {
            pc > 0 && (univ_total_count.get(*u).copied().unwrap_or(1) > 0)
        }).count() as i32;
    }

    println!("Run complete in {:.1}s", start_time.elapsed().as_secs_f64());

    // Write summary CSV
    let mut summary_csv = File::create("snapshots/chand_p_live_sweep_summary.csv")?;
    writeln!(summary_csv, "chand_p,pass_count,total_count,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,total_trades,positive_universes")?;

    let mut sorted_ps: Vec<usize> = sweep_values.clone();
    sorted_ps.sort_by(|&a, &b| {
        let sa = summary.get(&a).unwrap();
        let sb = summary.get(&b).unwrap();
        sb.pass_count.cmp(&sa.pass_count)
            .then_with(|| sb.avg_sharpe.partial_cmp(&sa.avg_sharpe).unwrap())
    });

    for &p in &sorted_ps {
        let s = summary.get(&p).unwrap();
        writeln!(summary_csv, "{},{},{},{:.4},{:.6},{:.4},{:.4},{},{}",
            p, s.pass_count, s.total_count,
            (s.pass_count as f64 / s.total_count as f64) * 100.0,
            s.avg_sharpe, s.avg_ret, s.avg_dd, s.total_trades, s.positive_universes)?;
    }
    println!("Summary written to snapshots/chand_p_live_sweep_summary.csv");

    // Print top 5
    println!("\nTop 5 CHAND_PERIOD values:");
    println!("{:>6} | {:>4}/{:>4} | {:>6} | {:>8} | {:>8} | {:>6}", "P", "pass", "total", "pass%", "Sharpe", "ret%", "DD%");
    println!("{}", "-".repeat(60));
    for &p in sorted_ps.iter().take(5) {
        let s = summary.get(&p).unwrap();
        println!("{:>6} | {:>4}/{:>4} | {:>6.2} | {:>8.4} | {:>8.2} | {:>6.2}",
            p, s.pass_count, s.total_count,
            (s.pass_count as f64 / s.total_count as f64) * 100.0,
            s.avg_sharpe, s.avg_ret, s.avg_dd);
    }

    // Export Base5 equity for top 5 P values
    let top5: Vec<usize> = sorted_ps.into_iter().take(5).collect();
    let mut equity_csv = File::create("snapshots/chand_p_live_selected_equity.csv")?;
    writeln!(equity_csv, "window,{}", top5.iter().map(|p| format!("P_{}", p)).collect::<Vec<_>>().join(","))?;
    for w in 0..windows {
        write!(equity_csv, "{}", w)?;
        for &p in &top5 {
            if let Some(agg) = equity_aggregates.get(&p) {
                if w < agg.len() {
                    write!(equity_csv, ",{:.6}", agg[w])?;
                } else {
                    write!(equity_csv, ",1.0")?;
                }
            }
        }
        writeln!(equity_csv)?;
    }
    println!("Equity written to snapshots/chand_p_live_selected_equity.csv");

    Ok(())
}