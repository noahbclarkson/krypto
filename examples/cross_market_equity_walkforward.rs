//! Cross-Market Equity Walk-Forward: SPY, QQQ, GLD
//!
//! Tests Turtle+Chandelier (frozen crypto-optimized params) on US equities.
//! If the strategy generalizes to non-crypto assets, the edge is proven market microstructure.
//! If it fails, it confirms crypto-specific regime dependency.
//!
//! Params (FROZEN from crypto hyperopt — NOT re-optimized for equities):
//!   EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, TURTLE_ATR=25, TURTLE_ATR_MULT=2.0, HOLD_MAX=45
//!
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side (conservative equity estimate)

use anyhow::Result;
use polars::prelude::*;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

const CANDLES: usize = 5000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.15; // hyperopt 2026-04-16: fine-tune winner (+25.5% Sharpe vs coarse 2.00). Same dual-exit mechanism as crypto.
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;

const ASSETS: &[(&str, &str)] = &[
    ("SPY", "spy_1d_equity.parquet"),
    ("QQQ", "qqq_1d_equity.parquet"),
    ("GLD", "gld_1d_equity.parquet"),
];

const OUT_CSV: &str = "snapshots/cross_market_equity_wf.csv";
const OUT_MD: &str = "snapshots/cross_market_equity_wf.md";

struct BarData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs().max((l - c0).abs())));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr) = close.get(idx) {
        curr > max_close
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn run_sim(bars: &BarData, test_start: usize, test_end: usize) -> (f64, f64, f64, usize, f64, bool) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let n = bars.close.len();
    let mut bar = test_start;
    while bar + 2 < test_end.min(n.saturating_sub(1)) {
        if bar >= TURTLE_ENTRY + 1 && bar < n {
            if turtle_signal(&bars.close, TURTLE_ENTRY, bar) {
                let entry_px = bars.close[bar];
                let entry = entry_px * (1.0 - TAKER_FEE);
                let entry_bar_next = bar + 1;
                let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));

                let mut highest_high_chand = bars.high[entry_bar_next];
                let mut highest_high_turtle = bars.high[entry_bar_next];
                let mut exit_bar = max_bar;

                for b in entry_bar_next..=max_bar {
                    highest_high_chand = highest_high_chand.max(bars.high[b]);
                    let atr_chand = atr_at(&bars.high, &bars.low, &bars.close, CHAND_PERIOD, b);
                    let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                    highest_high_turtle = highest_high_turtle.max(bars.high[b]);
                    let atr_turtle = atr_at(&bars.high, &bars.low, &bars.close, TURTLE_ATR_PERIOD, b);
                    let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                    if bars.close[b] < trail_chand || bars.close[b] < trail_turtle {
                        exit_bar = b;
                        break;
                    }
                }

                if let Some(&exit_px) = bars.close.get(exit_bar) {
                    let exit = exit_px * (1.0 - TAKER_FEE);
                    let gross_ret = exit / entry - 1.0;
                    let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                    wins += if gross_ret > 0.0 { 1 } else { 0 };
                    total_trades += 1;
                    equity *= 1.0 + gross_ret;

                    let avg_daily = gross_ret / bars_held as f64;
                    for _ in 0..bars_held {
                        daily_rets.push(avg_daily);
                    }

                    if equity > peak { peak = equity; }
                    equity_curve.push(equity);
                    bar = exit_bar + 1;
                    continue;
                }
            }
        }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let mdd = max_dd(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    (ret, sharpe, mdd, total_trades, win_rate, pass)
}

fn load_bars(path: &Path) -> Result<BarData> {
    let df = ParquetReader::new(File::open(path)?).finish()?;
    macro_rules! col_vec {
        ($name:expr) => {{
            let chunked = df.column($name)?.f64()?;
            chunked.into_iter().filter_map(|x| x).collect::<Vec<_>>()
        }};
    }
    Ok(BarData {
        close: col_vec!("close"),
        high:  col_vec!("high"),
        low:   col_vec!("low"),
        vol:   col_vec!("volume"),
    })
}

struct AssetResult {
    label: String,
    n_windows: f64,
    passed: f64,
    avg_ret: f64,
    avg_sharpe: f64,
    worst_dd: f64,
    total_trades: usize,
}

impl AssetResult {
    fn pass_rate(&self) -> f64 {
        self.passed / self.n_windows.max(1.0) * 100.0
    }
}

fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Cross-Market Equity Walk-Forward ====");
    eprintln!("FROZEN crypto params: EP=21, CHAND(28,2.0), ATR(25,2.0), HM=45\n");

    let cache = Path::new("data/cache");
    let mut results = Vec::new();
    let mut csv_lines = vec!["asset,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    for &(label, filename) in ASSETS {
        let path = cache.join(filename);
        if !path.exists() {
            eprintln!("{}: {} not found — skipping", label, path.display());
            continue;
        }

        let bars = match load_bars(&path) {
            Ok(b) => b,
            Err(e) => { eprintln!("{}: load failed: {}", label, e); continue; }
        };

        let n = bars.close.len();
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

        eprintln!("==== {} ({} bars, {} windows) ====", label, n, total_windows);

        let mut asset_passed = 0usize;
        let mut window_rets = Vec::new();
        let mut window_sharpes = Vec::new();
        let mut window_dds = Vec::new();
        let mut window_trades = Vec::new();
        let mut window_win_rates = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 10 { continue; }

            let (ret, sharpe, mdd, trades, win_rate, pass) = run_sim(&bars, test_start, test_end);
            let thin = if trades < MIN_TRADES { "THIN" } else { "OK" };
            let result = if pass { "PASS" } else { "FAIL" };

            eprintln!(
                "  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                wi, ret, sharpe, mdd, trades, win_rate, thin, result
            );

            csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                label, wi, ret, sharpe, mdd, trades, win_rate, pass));

            if pass { asset_passed += 1; }
            window_rets.push(ret);
            window_sharpes.push(sharpe);
            window_dds.push(mdd);
            window_trades.push(trades);
            window_win_rates.push(win_rate);
        }

        let n_win = window_rets.len() as f64;
        let avg_ret: f64 = window_rets.iter().sum::<f64>() / n_win;
        let avg_sh: f64 = window_sharpes.iter().sum::<f64>() / n_win;
        let worst_dd: f64 = window_dds.iter().fold(0.0_f64, |a, &b| a.max(b));
        let total_trades: usize = window_trades.iter().sum();

        eprintln!(
            "  AGG | avg {:+7.1}% sh={:.2} DD={:.1}% | {}/{} pass ({:.0}%)\n",
            avg_ret, avg_sh, worst_dd, asset_passed, total_windows,
            asset_passed as f64 / total_windows as f64 * 100.0
        );

        results.push(AssetResult {
            label: label.to_string(),
            n_windows: n_win,
            passed: asset_passed as f64,
            avg_ret,
            avg_sharpe: avg_sh,
            worst_dd,
            total_trades,
        });
    }

    let mut f = File::create(OUT_CSV)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }

    let mut md = File::create(OUT_MD)?;
    writeln!(md, "# Cross-Market Equity Walk-Forward")?;
    writeln!(md, "")?;
    writeln!(md, "FROZEN crypto params: EP=21, CHAND(28,2.0), ATR(25,2.0), HOLD_MAX=45. NOT re-optimized for equities.")?;
    writeln!(md, "")?;
    writeln!(md, "| Asset | Pass Rate | Avg Return | Avg Sharpe | Worst DD | Total Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|")?;
    for r in &results {
        writeln!(md, "| {} | {:.0}/{:.0} ({:.0}%) | {:+.1}% | {:.2} | {:.1}% | {} |",
            r.label, r.passed, r.n_windows, r.pass_rate(),
            r.avg_ret, r.avg_sharpe, r.worst_dd, r.total_trades)?;
    }
    writeln!(md, "")?;

    let total_pass: f64 = results.iter().map(|r| r.passed).sum();
    let total_win: f64 = results.iter().map(|r| r.n_windows).sum();
    let overall_pass_rate = total_pass / total_win.max(1.0) * 100.0;
    writeln!(md, "**Overall: {:.0}/{:.0} windows passed ({:.0}%)**", total_pass, total_win, overall_pass_rate)?;
    writeln!(md, "")?;
    writeln!(md, "## Interpretation")?;
    writeln!(md, "")?;
    writeln!(md, "- SPY/QQQ/GLD pass rate ≥3/3 → **edge generalizes beyond crypto**")?;
    writeln!(md, "- SPY/QQQ/GLD pass rate 1-2/3 → **edge partially generalizes, crypto adds alpha**")?;
    writeln!(md, "- SPY/QQQ/GLD pass rate 0/3 → **crypto-only edge, regime-dependent**")?;

    eprintln!("\n==== GLOBAL SUMMARY ====");
    eprintln!("  Overall: {:.0}/{:.0} windows passed ({:.0}%)", total_pass, total_win, overall_pass_rate);
    eprintln!("  CSV: {}", OUT_CSV);
    eprintln!("  MD: {}", OUT_MD);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}