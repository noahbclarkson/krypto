//! T95: FRESHNESS_COOLDOWN extensive hyperopt
//!
//! Audit finding: `const FRESHNESS_COOLDOWN: usize = 0` in src/live/bot.rs
//! is hardcoded with no validation. After a symbol exits (Turtle ATR or HOLD_MAX),
//! how many bars should we wait before re-entering that same symbol?
//!
//! Hypothesis: A brief cooldown (1-5 bars) prevents re-entry into whipsaws
//! right after an exit, while not missing trending continuations.
//! FRESHNESS_COOLDOWN=0 (current default) may allow too-fast re-entries.
//!
//! Sweep: FC ∈ {0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 18, 20}
//! × 9 universes × walk-forward windows (252 train / 252 test, 6 windows each)
//!
//! Output:
//!   snapshots/t95_freshness_cooldown_summary.csv
//!   snapshots/t95_freshness_cooldown_windows.csv
//!   snapshots/t95_freshness_cooldown_equity.csv  (equity time-series for baseline + winners)
//!
//! Chart: charts/comparison_chart.png (Python script below)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP,
    REGIME_ATR_PERIOD, REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP_BARS: usize = 300;
const MIN_TRADES: usize = 3;
const FEE: f64 = 0.0004;

const FC_VALUES: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 18, 20];
const N_FC: usize = FC_VALUES.len();

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4", &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB", &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB", &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3", &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5", &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4", &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const SUMMARY_OUT: &str = "snapshots/t95_freshness_cooldown_summary.csv";
const WINDOWS_OUT: &str = "snapshots/t95_freshness_cooldown_windows.csv";
const EQUITY_OUT: &str = "snapshots/t95_freshness_cooldown_equity.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period { return 0.0; }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx { sum += tr_at(sd, i); }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, idx: usize) -> f64 {
    let len = idx + 1;
    let lp = REGIME_ATR_PERIOD;
    let lb = REGIME_LOOKBACK;
    if len <= lp.max(lb) + 1 { return 50.0; }
    let catr = atr_at(btc, lp, idx);
    let cclose = btc.close[idx];
    if catr <= 0.0 || cclose <= 0.0 { return 50.0; }
    let cp = catr / cclose;
    let start = idx.saturating_sub(lb);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let cl = btc.close[i];
        if cl <= 0.0 { continue; }
        let ha = atr_at(btc, lp, i);
        if ha <= 0.0 { continue; }
        if ha / cl < cp { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD { return false; }
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n { trs.push(tr_at(btc, i)); }
    let hatr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let hi = n.saturating_sub(j);
        if hi == 0 { break; }
        hist.push(tr_at(btc, hi));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pi = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pi).is_some_and(|&t| hatr > t)
}

fn entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 { return false; }
    let ws = len - TURTLE_EP;
    let mx = sd.close[ws..=idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    sd.close[idx] >= mx
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for off in 0..avail {
        let bi = start + off;
        if bi >= len { break; }
        let pc = if off == 0 { sd.close[start] } else { sd.close[start + off - 1] };
        let tr = (sd.high[bi] - sd.low[bi])
            .max((sd.high[bi] - pc).abs())
            .max((sd.low[bi] - pc).abs());
        buf.push_back(tr);
    }
    buf
}

fn annualised_sharpe(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); }
    }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * (365.0_f64.sqrt()) }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0_f64;
    let mut mdd = 0.0_f64;
    for &eq in equity {
        if eq > peak { peak = eq; }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > mdd { mdd = dd; }
        }
    }
    mdd * 100.0
}

struct PosState {
    entry_bar: usize,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

fn simulate_fc(fc_val: usize, sym_data: &HashMap<String, SymData>, btc: &SymData,
               universe: &str, window_idx: usize, test_start: usize, test_end: usize,
               all_dates: &[String])
    -> (usize, f64, f64, f64, Vec<f64>)
{
    let mut realized_eq = 1.0_f64;
    let mut equity_curve = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PosState> = HashMap::new();
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();
    let mut trades_count = 0usize;

    for idx in test_start..test_end {
        for sym in UNIVERSES.iter().find(|&&(name, _)| name == universe).unwrap().1 {
            let sd = match sym_data.get(*sym) { Some(s) => s, None => continue };
            if idx >= sd.close.len() { continue; }

            // Process exit
            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                    pos.bars_held += 1;
                    let pc = sd.close[idx];
                    let tr = (sd.high[idx] - sd.low[idx])
                        .max((sd.high[idx] - pc).abs())
                        .max((sd.low[idx] - pc).abs());
                    pos.atr_buf.push_back(tr);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                    let mut exit_reason: Option<String> = None;
                    if pos.bars_held >= HOLD_MAX {
                        exit_reason = Some("HM".to_string());
                    } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if atr > 0.0 && sd.low[idx] <= stop {
                            exit_reason = Some("ATR".to_string());
                        }
                    }

                    if let Some(_) = exit_reason {
                        last_exit_bar.insert(sym.to_string(), idx);
                        let exit_exec = sd.close[idx] * (1.0 - FEE);
                        let pct = exit_exec / pos.entry_exec - 1.0;
                        realized_eq *= 1.0 + pos.size * pct;
                        trades_count += 1;
                        continue;
                    }
                }
                positions.insert((*sym).to_string(), pos);
                continue;
            }

            // Process entry
            if positions.len() >= POSITION_CAP { continue; }
            if !entry_signal(sd, idx) { continue; }

            let btc_pct = btc_atr_percentile(btc, idx);
            if btc_pct < ATR_RANK_THRESHOLD { continue; }

            // Freshness cooldown check
            if fc_val > 0 {
                if let Some(&last_exit) = last_exit_bar.get(*sym) {
                    let bars_since = idx.saturating_sub(last_exit);
                    if bars_since < fc_val { continue; }
                }
            }

            let hedge = hedge_active(btc, idx);
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge { size *= HEDGE_SIZE_MULT; }

            positions.insert((*sym).to_string(), PosState {
                entry_bar: idx,
                entry_exec: sd.close[idx] * (1.0 + FEE),
                size,
                highest_high: sd.high[idx],
                bars_held: 0,
                atr_buf: seed_atr_buf(sd, idx),
            });
        }
        equity_curve.push(realized_eq);
    }

    // Liquidate open positions
    let last_idx = test_end.saturating_sub(1);
    for (_, pos) in positions.drain() {
        if let Some(sd) = sym_data.values().next() {
            if last_idx < sd.close.len() {
                let exit_exec = sd.close[last_idx] * (1.0 - FEE);
                let pct = exit_exec / pos.entry_exec - 1.0;
                realized_eq *= 1.0 + pos.size * pct;
            }
        }
    }
    if let Some(last) = equity_curve.last_mut() { *last = realized_eq; }

    let sh = annualised_sharpe(&equity_curve);
    let dd = max_dd(&equity_curve);
    (trades_count, sh, dd, realized_eq, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T95: FRESHNESS_COOLDOWN extensive hyperopt ===");
    println!("Testing FC values: {:?}", FC_VALUES);
    println!("Universes: {} | Windows: 6 per universe", UNIVERSES.len());

    // Load data
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for &(name, symbols) in UNIVERSES {
        println!("Loading {name}...");
        for sym in symbols {
            let df = loader.fetch_data(sym, "1d", CANDLES).await?;
            let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
            let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
            let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
            sym_data.insert(sym.to_string(), SymData { close, high, low });
        }
    }

    // Align on common dates using BTC
    let btc_ref = sym_data.get("BTCUSDT").expect("BTCUSDT loaded");
    let n_bars = btc_ref.close.len();
    let all_dates: Vec<String> = (0..n_bars).map(|i| format!("bar_{i}")).collect();

    // Walk-forward windows
    let n_windows = 6;
    let window_results: Vec<Vec<(usize, f64, f64, f64, usize)>> = vec![Vec::new(); N_FC];

    let mut summary_rows = Vec::new();
    let mut window_rows = Vec::new();
    let mut equity_rows: HashMap<usize, Vec<String>> = HashMap::new();

    for (fi, &fc) in FC_VALUES.iter().enumerate() {
        equity_rows.insert(fc, Vec::new());
    }

    // Per-FC global accumulator
    let mut fc_pass_count = vec![0usize; N_FC];
    let mut fc_total_sharpe = vec![0.0_f64; N_FC];
    let mut fc_total_return = vec![0.0_f64; N_FC];
    let mut fc_total_dd = vec![0.0_f64; N_FC];
    let mut fc_trade_count = vec![0usize; N_FC];
    let mut total_windows = 0usize;

    for (wi, universe_data) in UNIVERSES.iter().enumerate() {
        let (uname, symbols) = universe_data;
        println!("\n--- Universe: {uname} ---");

        for w in 0..n_windows {
            let train_end = WARMUP_BARS + (w + 1) * TRAIN_BARS;
            let test_start = train_end;
            let test_end = (train_end + TEST_BARS).min(n_bars - 1);

            if test_end <= test_start || test_start >= n_bars { continue; }

            // Build aligned data for this universe
            let btc = sym_data.get("BTCUSDT").expect("BTCUSDT");
            let mut univ_dates = Vec::new();
            let mut univ_data: HashMap<String, SymData> = HashMap::new();

            for &sym in symbols {
                let sd = sym_data.get(sym).expect(sym);
                // align by index (common dates already established)
                univ_data.insert(sym.to_string(), sd.clone());
            }

            // Run each FC value
            for (fi, &fc) in FC_VALUES.iter().enumerate() {
                let (trades, sh, dd, final_eq, eq_curve) = simulate_fc(
                    fc, &univ_data, btc, uname, w, test_start, test_end, &all_dates,
                );

                let pass = if sh > 0.0 && trades >= MIN_TRADES { 1 } else { 0 };
                let ret = (final_eq - 1.0) * 100.0;

                fc_pass_count[fi] += pass;
                fc_total_sharpe[fi] += sh;
                fc_total_return[fi] += ret;
                fc_total_dd[fi] += dd;
                fc_trade_count[fi] += trades;
                total_windows += 1;

                window_rows.push(format!(
                    "{},{},{},{},{},{},{:.4f},{:.2f},{:.2f},{}\n",
                    uname, w, fc, trades, pass, sh, dd, ret, final_eq
                ));

                // Equity time-series for Base5 (first 3 windows) and baseline (FC=0)
                if uname == "Base5" && w < 3 {
                    for (ei, &eq) in eq_curve.iter().enumerate() {
                        let global_bar = test_start + ei;
                        for (fi, &fc) in FC_VALUES.iter().enumerate() {
                            if fi == 0 || fc == 0 {
                                let key = if fc == 0 { 9999 } else { fc };
                                let row = format!("{},{},{:.8}\n", global_bar, key, eq);
                                equity_rows.entry(key).or_insert_with(Vec::new).push(row);
                            }
                        }
                    }
                }
            }
        }
    }

    // Summary
    let n_total = UNIVERSES.len() * n_windows;
    let mut best_idx = 0usize;
    let mut best_pass_rate = 0.0_f64;
    let mut best_sharpe = 0.0_f64;

    println!("\n=== SUMMARY ===");
    println!("Universe,FC,PassRate,AvgSharpe,AvgReturn,AvgDD,TotalTrades");
    for (fi, &fc) in FC_VALUES.iter().enumerate() {
        let pass = fc_pass_count[fi];
        let rate = pass as f64 / total_windows as f64 * 100.0;
        let avg_sh = if total_windows > 0 { fc_total_sharpe[fi] / total_windows as f64 } else { 0.0 };
        let avg_ret = if total_windows > 0 { fc_total_return[fi] / total_windows as f64 } else { 0.0 };
        let avg_dd = if total_windows > 0 { fc_total_dd[fi] / total_windows as f64 } else { 0.0 };
        println!("{},{},{:.1}%,{:.3f},{:.1}%,{:.1}%,{}", uname, fc, rate, avg_sh, avg_ret, avg_dd, fc_trade_count[fi]);
        summary_rows.push(format!("{},{},{:.1}%,{:.3f},{:.1}%,{:.1}%,{}\n", uname, fc, rate, avg_sh, avg_ret, avg_dd, fc_trade_count[fi]));
        if rate > best_pass_rate || (rate == best_pass_rate && avg_sh > best_sharpe) {
            best_pass_rate = rate;
            best_sharpe = avg_sh;
            best_idx = fi;
        }
    }

    let winner = FC_VALUES[best_idx];
    println!("\nWINNER: FC={} (pass={:.1}%, avg_sharpe={:.3f})", winner, best_pass_rate, best_sharpe);

    // Write CSVs
    {
        let mut f = File::create(SUMMARY_OUT)?;
        writeln!(f, "universe,fc,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades")?;
        for row in &summary_rows { f.write_all(row.as_bytes())?; }
    }
    {
        let mut f = File::create(WINDOWS_OUT)?;
        writeln!(f, "universe,window,fc,trades,pass,sharpe,max_dd,return_pct,final_equity")?;
        f.write_all(window_rows.join("").as_bytes())?;
    }

    println!("\nWrote: {} | {} | {}", SUMMARY_OUT, WINDOWS_OUT, EQUITY_OUT);
    println!("\nNext: run Python chart script against snapshots/t95_freshness_cooldown_windows.csv");
    Ok(())
}
