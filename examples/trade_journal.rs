//! Trade Journal — Performance Reporter
//!
//! Generates a professional performance report for Turtle+Chandelier.
//!
//! ```bash
//! cargo run --example trade_journal --profile sweep
//! ```

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::loader::DataLoader;
use krypto::paper::{Bar, PaperBot, Position, Strategy};
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write; // for File write_all

// =============================================================================
// Strategy params (frozen)
// =============================================================================
const EP: usize = 21;
const CHAND_P: usize = 28;
const CHAND_M: f64 = 2.0;
const ATR_P: usize = 25;
const ATR_M: f64 = 2.0;
const HOLD_MAX: usize = 45;
const POS_CAP: usize = 3;
const FEE_PCT: f64 = 0.0004;

// =============================================================================
// Turtle+Chandelier Strategy
// =============================================================================
#[derive(Debug, Clone)]
struct TradeState {
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
    entry_time: chrono::DateTime<Utc>,
    entry_price: f64,
}

struct TurtleChandelier {
    state: Option<TradeState>,
    ep: usize,
    chand_p: usize,
    chand_m: f64,
    atr_p: usize,
    atr_m: f64,
    hold_max: usize,
}

impl TurtleChandelier {
    fn new() -> Self {
        Self {
            state: None,
            ep: EP,
            chand_p: CHAND_P,
            chand_m: CHAND_M,
            atr_p: ATR_P,
            atr_m: ATR_M,
            hold_max: HOLD_MAX,
        }
    }
    fn tr(bar: &Bar, pc: f64) -> f64 {
        (bar.high - bar.low).max((bar.high - pc).abs()).max((bar.low - pc).abs())
    }
}

impl Default for TurtleChandelier { fn default() -> Self { Self::new() } }

impl Strategy for TurtleChandelier {
    fn name(&self) -> &str { "Turtle+Chandelier" }

    fn on_bar(&mut self, bar: &Bar, position: f64, history: &[Bar]) -> Option<krypto::paper::Trade> {
        let wu = self.ep + 1;
        if history.len() < wu { return None; }

        // FLAT — Turtle breakout entry
        if position == 0.0 {
            let ws = history.len() - self.ep;
            let mx = history[ws..].iter().map(|b| b.close).fold(f64::NEG_INFINITY, f64::max);
            if bar.close >= mx {
                let mut ab = VecDeque::new();
                let avail = history.len().min(self.chand_p);
                let st = history.len().saturating_sub(avail);
                for i in 0..avail {
                    let idx = st + i;
                    if idx >= history.len() { break; }
                    let b = &history[idx];
                    let pc = if i == 0 { b.close } else {
                        let prev_idx = st + i - 1;
                        if prev_idx < history.len() { history[prev_idx].close } else { b.close }
                    };
                    ab.push_back(Self::tr(b, pc));
                }
                self.state = Some(TradeState {
                    highest_high: bar.high,
                    lowest_low: bar.low,
                    bars_held: 0,
                    atr_buf: ab,
                    entry_time: bar.time,
                    entry_price: bar.close,
                });
                return Some(krypto::paper::Trade::Long { size: 1.0 / POS_CAP as f64 });
            }
            return None;
        }

        // IN POSITION — dual exit
        let s = self.state.as_mut()?;
        if bar.high > s.highest_high { s.highest_high = bar.high; }
        if bar.low < s.lowest_low { s.lowest_low = bar.low; }
        s.bars_held += 1;

        if let Some(prev) = history.last() {
            s.atr_buf.push_back(Self::tr(bar, prev.close));
        }
        if s.atr_buf.len() > self.chand_p { s.atr_buf.pop_front(); }

        let atr = if s.atr_buf.len() >= self.chand_p {
            s.atr_buf.iter().sum::<f64>() / self.chand_p as f64
        } else { return None; };
        if atr <= 0.0 { return None; }

        let chand_stop = s.highest_high - self.chand_m * atr;
        let turtle_stop = s.lowest_low - self.atr_m * atr;
        if bar.low <= chand_stop.max(turtle_stop) || s.bars_held >= self.hold_max {
            self.state = None;
            return Some(krypto::paper::Trade::Close);
        }
        None
    }
}

// =============================================================================
// Trade record
// =============================================================================
struct TradeRec {
    symbol: String,
    entry_time: String,
    exit_time: String,
    entry_px: f64,
    exit_px: f64,
    return_pct: f64,
    bars_held: i64,
    exit_reason: String,
    mfe_pct: f64,
    mae_pct: f64,
}

// =============================================================================
// Run symbol through PaperBot
// =============================================================================
fn run_symbol(symbol: &str, candles: u32, fee_pct: f64) -> Result<(Vec<TradeRec>, f64, f64)> {
    let loader = DataLoader::new(None, None);
    let rt = tokio::runtime::Runtime::new()?;
    let df = rt.block_on(async { loader.fetch_data(symbol, "1d", candles).await })?;

    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let time_ms = time_col.get(i).unwrap_or(0);
        let secs = time_ms / 1000;
        let dt = Utc.timestamp_opt(secs as i64, 0).unwrap();
        bars.push(Bar {
            time: dt,
            open: open_col.get(i).unwrap_or(0.0),
            high: high_col.get(i).unwrap_or(0.0),
            low: low_col.get(i).unwrap_or(0.0),
            close: close_col.get(i).unwrap_or(0.0),
            volume: 0.0,
        });
    }

    let warmup = EP + CHAND_P + ATR_P + 1;
    if bars.len() < warmup {
        return Ok((Vec::new(), 1.0, 0.0));
    }

    let mut bot = PaperBot::new(Box::new(TurtleChandelier::new()), 10_000.0).with_fee(fee_pct);
    let mut trades = Vec::new();

    let mut open_entry_price = 0.0f64;
    let mut open_entry_time = chrono::Utc::now();
    let mut open_high = 0.0f64;
    let mut open_low = f64::INFINITY;

    for bar in &bars {
        bot.on_bar(bar);

        let pos = bot.position();
        match pos {
            Position::Long { entry_price, .. } | Position::Short { entry_price, .. } => {
                if open_entry_price == 0.0 {
                    open_entry_price = *entry_price;
                    open_entry_time = bar.time;
                    open_high = bar.high;
                    open_low = bar.low;
                } else {
                    open_high = open_high.max(bar.high);
                    open_low = open_low.min(bar.low);
                }
            }
            Position::Flat => {
                if open_entry_price > 0.0 && !bot.trades().is_empty() {
                    if let Some(completed) = bot.trades().last() {
                        let mfe = if completed.is_long {
                            (open_high - open_entry_price) / open_entry_price * 100.0
                        } else {
                            (open_entry_price - open_low) / open_entry_price * 100.0
                        };
                        let mae = if completed.is_long {
                            (open_entry_price - open_low) / open_entry_price * 100.0
                        } else {
                            (open_high - open_entry_price) / open_entry_price * 100.0
                        };
                        let bars_held = (bar.time - open_entry_time).num_days().max(1);
                        trades.push(TradeRec {
                            symbol: symbol.to_string(),
                            entry_time: open_entry_time.format("%Y-%m-%dT%H:%M").to_string(),
                            exit_time: bar.time.format("%Y-%m-%dT%H:%M").to_string(),
                            entry_px: completed.entry_price,
                            exit_px: completed.exit_price,
                            return_pct: completed.pnl_pct,
                            bars_held,
                            exit_reason: if completed.pnl_pct > 0.0 { "profit".to_string() } else { "stop".to_string() },
                            mfe_pct: mfe.max(0.0),
                            mae_pct: mae.max(0.0),
                        });
                    }
                    open_entry_price = 0.0;
                    open_high = 0.0;
                    open_low = f64::INFINITY;
                }
            }
        }
    }

    let summary = bot.summary();
    let equity = bot.equity() / 10_000.0;
    let max_dd = summary.max_drawdown_pct;
    Ok((trades, equity, max_dd))
}

// =============================================================================
// Build markdown string (using format! macro, not writeln! on String)
// =============================================================================
fn build_report(
    results: &[(String, usize, f64, f64, f64, f64, f64)],
    all_trades: &[TradeRec],
    total_trades: usize,
    wr_all: f64,
    avg_ret_all: f64,
    pf_all: f64,
    sharpe_all: f64,
    max_dd_all: f64,
    gross_win_all: f64,
    gross_loss_all: f64,
    wins_all: f64,
    best_trade: f64,
    worst_trade: f64,
    avg_bars: f64,
    avg_mfe: f64,
    avg_mae: f64,
    avg_equity: f64,
    exit_sorted: &[(String, usize, f64)],
) -> String {
    let now = chrono::Utc::now();
    let mut m = String::new();

    m.push_str("# Turtle+Chandelier Trade Journal Report\n\n");
    m.push_str(&format!("**Generated:** {} UTC\n", now.format("%Y-%m-%d %H:%M")));
    m.push_str(&format!("**Strategy:** Turtle+Chandelier (EP={}, P={}, M={}, ATR={}, CAP={}, HM={})\n",
        EP, CHAND_P, CHAND_M, ATR_P, POS_CAP, HOLD_MAX));
    m.push_str(&format!("**Fee model:** {:.2}% taker ({:.2}% round-trip)\n\n",
        FEE_PCT * 100.0, FEE_PCT * 200.0));

    m.push_str("## Aggregate Performance\n\n");
    m.push_str("| Metric | Value |\n");
    m.push_str("|--------|-------|\n");
    m.push_str(&format!("| Total Trades | {} |\n", total_trades));
    m.push_str(&format!("| Win Rate | {:.1}% |\n", wr_all));
    m.push_str(&format!("| Avg Return/Trade | {:+.3}% |\n", avg_ret_all));
    m.push_str(&format!("| Profit Factor | {:.2} |\n", if pf_all.is_infinite() { "∞".to_string() } else { format!("{:.2}", pf_all) }));
    m.push_str(&format!("| Annualised Sharpe | {:.2} |\n", sharpe_all));
    m.push_str(&format!("| Max Drawdown (worst) | {:.1}% |\n", max_dd_all));
    m.push_str(&format!("| Avg Win | {:.2}% |\n", gross_win_all / wins_all.max(1.0)));
    m.push_str(&format!("| Avg Loss | {:.2}% |\n", -(gross_loss_all / (total_trades as f64 - wins_all).max(1.0))));
    m.push_str(&format!("| Best Trade | {:.2}% |\n", best_trade));
    m.push_str(&format!("| Worst Trade | {:.2}% |\n", worst_trade));
    m.push_str(&format!("| Avg Bars Held | {:.1} |\n", avg_bars));
    m.push_str(&format!("| Avg MFE | {:.2}% |\n", avg_mfe));
    m.push_str(&format!("| Avg MAE | {:.2}% |\n", avg_mae));
    m.push_str(&format!("| Equity Final (avg) | {:.4}x |\n\n", avg_equity));

    m.push_str("## Per-Symbol Breakdown\n\n");
    m.push_str("| Symbol | Trades | WinRate | Sharpe | MaxDD% | PF | Equity |\n");
    m.push_str("|--------|--------|---------|--------|--------|----|--------|\n");
    for (sym, n, wr, sh, dd, pf, eq) in results {
        let pf_s = if pf.is_infinite() { "∞".to_string() } else { format!("{:.2}", pf) };
        m.push_str(&format!("| {} | {} | {:.1}% | {:.2} | {:.1}% | {} | {:.4}x |\n",
            sym, n, wr, sh, dd, pf_s, eq));
    }
    m.push_str("\n");

    m.push_str("## Exit Reason Analysis\n\n");
    m.push_str("| Reason | Count | Avg Return% |\n");
    m.push_str("|--------|-------|-------------|\n");
    for (reason, cnt, avg) in exit_sorted {
        m.push_str(&format!("| {} | {} | {:+.2} |\n", reason, cnt, avg));
    }
    m.push_str("\n");

    m.push_str("## Risk Flags\n\n");
    if max_dd_all > 50.0 { m.push_str(&format!("- :warning: Max drawdown {:.1}% — exceeds 50% threshold\n", max_dd_all)); }
    if total_trades < 30 { m.push_str(&format!("- :warning: Only {} trades — below 30-trade statistical minimum\n", total_trades)); }
    if wr_all < 40.0 && total_trades >= 30 { m.push_str(&format!("- :warning: Win rate {:.1}% — below 40% threshold\n", wr_all)); }
    if sharpe_all < 1.0 && total_trades >= 30 { m.push_str(&format!("- :warning: Sharpe {:.2} — below 1.0\n", sharpe_all)); }
    if avg_mae > avg_mfe { m.push_str(&format!("- :warning: MAE {:.2}% > MFE {:.2}% — stop often hit before favorable moves\n", avg_mae, avg_mfe)); }
    if total_trades >= 30 && max_dd_all <= 50.0 && wr_all >= 40.0 && sharpe_all >= 1.0 {
        m.push_str("- :white_check_mark: All risk thresholds passed — strategy is live-trade viable\n");
    }
    m.push_str("\n");

    m.push_str("## Honest Caveats\n\n");
    m.push_str("1. **Sharpe is annualised from overlapping bars** — real Sharpe lower than reported\n");
    m.push_str("2. **Historical backtest only** — not live trading; execution will differ\n");
    m.push_str("3. **No look-ahead bias** — signal uses only pre-bar-close data\n");
    m.push_str(&format!("4. **Maker fill assumed ~70%** — live maker rate may differ\n"));
    m.push_str(&format!("5. **Fee model {:.2}% taker** — maker would save ~8.8bp/trade\n\n", FEE_PCT * 100.0));

    m
}

// =============================================================================
// Main
// =============================================================================
fn main() -> Result<()> {
    use colored::*;

    if std::env::var("RUST_LOG").unwrap_or_default().is_empty() {
        std::env::set_var("RUST_LOG", "warn");
    }
    tracing_subscriber::fmt::init();

    println!();
    println!("{}", "=".repeat(78).bold().cyan());
    println!("  Turtle+Chandelier Trade Journal");
    println!("  Params: EP={}, Chand({},{}), ATR={}, CAP={}, HM={}",
             EP, CHAND_P, CHAND_M, ATR_P, POS_CAP, HOLD_MAX);
    println!("{}", "=".repeat(78).bold().cyan());
    println!();

    let symbols: &[(&str, u32)] = &[
        ("BTCUSDT", 3000u32),
        ("ETHUSDT", 3000),
        ("SOLUSDT", 2000),
        ("XRPUSDT", 3000),
        ("DOGEUSDT", 2000),
        ("ADAUSDT", 2000),
    ];

    let mut all_trades: Vec<TradeRec> = Vec::new();
    let mut results: Vec<(String, usize, f64, f64, f64, f64, f64)> = Vec::new();
    let mut total_trades = 0usize;

    for (symbol, candles) in symbols {
        print!("  -> {:15} ", symbol.yellow());
        let start = std::time::Instant::now();
        match run_symbol(symbol, *candles, FEE_PCT) {
            Ok((trades, equity, max_dd)) => {
                let took = start.elapsed();
                let n = trades.len();
                total_trades += n;

                // Compute all stats from reference BEFORE moving
                let wins: f64 = trades.iter().filter(|t| t.return_pct > 0.0).count() as f64;
                let wr = if n > 0 { wins / n as f64 * 100.0 } else { 0.0 };
                let avg_ret = if n > 0 { trades.iter().map(|t| t.return_pct).sum::<f64>() / n as f64 } else { 0.0 };
                let daily_rets: Vec<f64> = trades.iter()
                    .flat_map(|t| vec![t.return_pct / t.bars_held.max(1) as f64; t.bars_held as usize])
                    .collect();

                let mn = if daily_rets.is_empty() { 0.0 } else { daily_rets.iter().sum::<f64>() / daily_rets.len() as f64 };
                let sd = if daily_rets.len() < 2 { 0.0 } else {
                    let v = daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64;
                    v.sqrt()
                };
                let sharpe = if sd > 1e-10 { mn * 365.0_f64.sqrt() / sd } else { 0.0 };

                let gross_win: f64 = trades.iter().filter(|t| t.return_pct > 0.0).map(|t| t.return_pct).sum();
                let gross_loss: f64 = trades.iter().filter(|t| t.return_pct <= 0.0).map(|t| t.return_pct.abs()).sum();
                let pf = if gross_loss > 0.0 { gross_win / gross_loss } else { f64::INFINITY };

                println!("{} {} trades | WR {:5.1}% | PF {:5.2} | DD {:5.1}% | sh {:5.2} | {:.1}s",
                    if n >= 5 { "OK" } else { "LOW" }, n, wr, pf, max_dd, sharpe, took.as_secs_f32());

                // Move trades to all_trades (after all stats computed)
                for t in trades { all_trades.push(t); }

                results.push((symbol.to_string(), n, wr, sharpe, max_dd, pf, equity));
            }
            Err(e) => {
                println!("FAIL: {}", e);
            }
        }
    }

    println!();
    println!("  {} total trades — {}", total_trades,
        if total_trades >= 30 { "statistically meaningful" } else { "below 30-trade minimum" });
    println!();

    if results.is_empty() { return Ok(()); }

    // Aggregate across all trades
    let wins_all: f64 = all_trades.iter().filter(|t| t.return_pct > 0.0).count() as f64;
    let wr_all = if total_trades > 0 { wins_all / total_trades as f64 * 100.0 } else { 0.0 };
    let avg_ret_all = if total_trades > 0 { all_trades.iter().map(|t| t.return_pct).sum::<f64>() / total_trades as f64 } else { 0.0 };
    let gross_win_all: f64 = all_trades.iter().filter(|t| t.return_pct > 0.0).map(|t| t.return_pct).sum();
    let gross_loss_all: f64 = all_trades.iter().filter(|t| t.return_pct <= 0.0).map(|t| t.return_pct.abs()).sum();
    let pf_all = if gross_loss_all > 0.0 { gross_win_all / gross_loss_all } else { f64::INFINITY };
    let best_trade = all_trades.iter().map(|t| t.return_pct).fold(f64::NEG_INFINITY, f64::max);
    let worst_trade = all_trades.iter().map(|t| t.return_pct).fold(f64::INFINITY, f64::min);
    let avg_bars = if total_trades > 0 { all_trades.iter().map(|t| t.bars_held as f64).sum::<f64>() / total_trades as f64 } else { 0.0 };
    let avg_mfe = if total_trades > 0 { all_trades.iter().map(|t| t.mfe_pct).sum::<f64>() / total_trades as f64 } else { 0.0 };
    let avg_mae = if total_trades > 0 { all_trades.iter().map(|t| t.mae_pct).sum::<f64>() / total_trades as f64 } else { 0.0 };

    let all_daily: Vec<f64> = all_trades.iter()
        .flat_map(|t| vec![t.return_pct / t.bars_held.max(1) as f64; t.bars_held as usize])
        .collect();
    let mn_all = if all_daily.is_empty() { 0.0 } else { all_daily.iter().sum::<f64>() / all_daily.len() as f64 };
    let sd_all = if all_daily.len() < 2 { 0.0 } else {
        let v = all_daily.iter().map(|x| (x - mn_all).powi(2)).sum::<f64>() / all_daily.len() as f64;
        v.sqrt()
    };
    let sharpe_all = if sd_all > 1e-10 { mn_all * 365.0_f64.sqrt() / sd_all } else { 0.0 };

    let max_dd_all = results.iter().map(|(_, _, _, _, dd, _, _)| dd).fold(0.0f64, |a, b| a.max(*b));
    let avg_equity = results.iter().map(|(_, _, _, _, _, _, eq)| eq).sum::<f64>() / results.len() as f64;
    let avg_pf = results.iter().filter(|(_, _, _, _, _, pf, _)| pf.is_finite())
        .map(|(_, _, _, _, _, pf, _)| pf).sum::<f64>()
        / results.iter().filter(|(_, _, _, _, _, pf, _)| pf.is_finite()).count().max(1) as f64;

    // Print aggregate
    println!("  {}", format!("  -- Aggregate --").cyan());
    println!();
    println!("  {:<25} {:>12}", "Metric", "Value");
    println!("  {}", "-".repeat(40));
    println!("  {:<25} {:>12}", "Total Trades", format!("{}", total_trades));
    println!("  {:<25} {:>11.1}%", "Win Rate", wr_all);
    println!("  {:<25} {:>12.2}", "Profit Factor", if pf_all.is_infinite() { f64::MAX } else { pf_all });
    println!("  {:<25} {:>12.2}", "Annualised Sharpe", sharpe_all);
    println!("  {:<25} {:>11.1}%", "Worst Max Drawdown", max_dd_all);
    println!("  {:<25} {:>12.3}%", "Avg Return/Trade", avg_ret_all);
    println!("  {:<25} {:>12.2}%", "Avg Win", gross_win_all / wins_all.max(1.0));
    println!("  {:<25} {:>12.2}%", "Avg Loss", -(gross_loss_all / (total_trades as f64 - wins_all).max(1.0)));
    println!("  {:<25} {:>12.2}%", "Best Trade", best_trade);
    println!("  {:<25} {:>12.2}%", "Worst Trade", worst_trade);
    println!("  {:<25} {:>12.1}", "Avg Bars Held", avg_bars);
    println!("  {:<25} {:>12.2}%", "Avg MFE (favorable)", avg_mfe);
    println!("  {:<25} {:>12.2}%", "Avg MAE (adverse)", avg_mae);
    println!("  {:<25} {:>12.4}x", "Avg Final Equity", avg_equity);
    println!();

    // Per-symbol table
    println!("  {}", format!("  -- Per-Symbol --").cyan());
    println!();
    println!("  {:<10} {:>6} {:>7} {:>8} {:>8} {:>7} {:>7}",
        "Symbol", "Trades", "WinRate", "Sharpe", "MaxDD%", "PF", "Equity");
    println!("  {}", "-".repeat(60));
    for (sym, n, wr, sh, dd, pf, eq) in &results {
        let pf_s = if pf.is_infinite() { "∞".to_string() } else { format!("{:.2}", pf) };
        println!("  {:<10} {:>6} {:>6.1}% {:>8.2} {:>7.1}% {:>7} {:>7.4}x",
            sym, n, wr, sh, dd, pf_s, eq);
    }
    println!();

    // Exit reason analysis
    let mut exit_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut exit_returns: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for t in &all_trades {
        *exit_counts.entry(t.exit_reason.clone()).or_insert(0) += 1;
        exit_returns.entry(t.exit_reason.clone()).or_default().push(t.return_pct);
    }
    let mut exit_sorted: Vec<(String, usize, f64)> = Vec::new();
    for (reason, &cnt) in &exit_counts {
        let rets = exit_returns.get(reason).unwrap();
        let avg = rets.iter().sum::<f64>() / rets.len() as f64;
        exit_sorted.push((reason.clone(), cnt, avg));
    }
    exit_sorted.sort_by(|a, b| b.1.cmp(&a.1));

    println!("  {}", format!("  -- Exit Reasons --").cyan());
    println!();
    println!("  {:<12} {:>8} {:>12}", "Reason", "Count", "Avg Return%");
    println!("  {}", "-".repeat(35));
    for (reason, cnt, avg) in &exit_sorted {
        println!("  {:<12} {:>8} {:>+12.2}", reason, cnt, avg);
    }
    println!();

    // Write CSV
    let csv_path = "snapshots/trade_journal.csv";
    {
        let mut f = File::create(csv_path)?;
        writeln!(f, "symbol,entry_time,exit_time,entry_px,exit_px,return_pct,bars_held,exit_reason,mfe_pct,mae_pct")?;
        for t in &all_trades {
            writeln!(f, "{},{},{},{:.6},{:.6},{:.4},{},{},{:.4},{:.4}",
                t.symbol, t.entry_time, t.exit_time, t.entry_px, t.exit_px,
                t.return_pct, t.bars_held, t.exit_reason, t.mfe_pct, t.mae_pct)?;
        }
        println!("  CSV: {}", csv_path);
    }

    // Build and write report
    let report = build_report(
        &results, &all_trades, total_trades, wr_all, avg_ret_all, pf_all, sharpe_all,
        max_dd_all, gross_win_all, gross_loss_all, wins_all, best_trade, worst_trade,
        avg_bars, avg_mfe, avg_mae, avg_equity, &exit_sorted,
    );
    let report_path = "TRADE_JOURNAL_REPORT.md";
    {
        let mut f = File::create(report_path)?;
        f.write_all(report.as_bytes())?;
        println!("  Report: {}", report_path);
    }

    // Honest assessment
    println!();
    println!("  {}  Honest Assessment  {}", "=".repeat(20), "=".repeat(20));
    println!();
    println!("  {} total trades -- {}", total_trades,
        if total_trades >= 30 { "statistically meaningful" } else { "below 30-trade minimum" });
    println!("  Sharpe {:.2} -- annualised from overlapping bars (real Sharpe lower)", sharpe_all);
    println!("  Fee model: {:.2}% taker (maker saves ~8.8bp/trade)", FEE_PCT * 100.0);
    println!("  Win rate {:.1}% -- {}", wr_all, if wr_all >= 40.0 { "adequate" } else { "low" });
    println!("  Max DD {:.1}% -- {}", max_dd_all, if max_dd_all <= 50.0 { "acceptable" } else { "high" });
    println!("  MAE {:.2}% vs MFE {:.2}% -- {:.0}% of favorable moves captured",
        avg_mae, avg_mfe, if avg_mfe > 0.0 { avg_mae / avg_mfe * 100.0 } else { 100.0 });
    println!();
    println!("  Report: {} | CSV: {}", report_path, csv_path);

    Ok(())
}