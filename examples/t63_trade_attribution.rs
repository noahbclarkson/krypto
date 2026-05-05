//! T63: Per-Trade PnL Attribution
//!
//! Asks: Is Turtle edge from few giant wins (fragile) or many small edges (robust)?
//! Also measures: fee impact, win/loss distribution, exit-type PnL, equity concentration.
//!
//! Matches `turtle_only_equity.rs` exactly — bar-by-bar, dollar-volume ranking,
//! REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=42, ATR_RANK_T=5.0, VOL_LOOKBACK=92.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 92;
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;
const TAKER_FEE: f64 = 0.001;

const SYMBOLS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn true_range(h: f64, l: f64, pc: f64) -> f64 {
    (h - l).max((h - pc).abs()).max((l - pc).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        sum += true_range(high[i], low[i], close[i.saturating_sub(1)]);
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        return 0.0;
    }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn btc_atr_pct(
    btc_close: &[f64],
    btc_high: &[f64],
    btc_low: &[f64],
    period: usize,
    lookback: usize,
    idx: usize,
) -> f64 {
    if idx < lookback + period {
        return 50.0;
    }
    let curr = atr_at(btc_high, btc_low, btc_close, period, idx);
    if curr <= 0.0 {
        return 50.0;
    }
    let start = idx + 1 - lookback - period;
    let end = idx + 1 - period;
    if end <= start {
        return 50.0;
    }
    let below = (start..=end)
        .filter(|&i| atr_at(btc_high, btc_low, btc_close, period, i) < curr)
        .count();
    below as f64 / (end - start + 1) as f64 * 100.0
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period {
        return false;
    }
    let start = idx - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    close.get(idx).copied().is_some_and(|c| c > max_close)
}

// ── Trade record ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Trade {
    symbol: String,
    entry_bar: usize,
    exit_bar: usize,
    bars_held: usize,
    entry_price: f64,
    exit_price: f64,
    gross_pct: f64,
    net_pct: f64,
    fee_pct: f64,
    by_turtle: bool,
    by_maxhold: bool,
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("T63: Per-Trade PnL Attribution\n");

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in &SYMBOLS {
        print!("  Loading {}...", sym);
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
        println!(" {} bars", close.len());
        let len = close.len();
        min_len = min_len.min(len);
        sym_data.insert(
            sym.to_string(),
            SymData {
                close,
                high,
                low,
                vol,
            },
        );
    }

    // BTC for regime filter
    let btc_df = loader.fetch_data("BTCUSDT", "1d", CANDLES).await?;
    let btc_close: Vec<f64> = btc_df.column("close")?.f64()?.into_no_null_iter().collect();
    let btc_high: Vec<f64> = btc_df.column("high")?.f64()?.into_no_null_iter().collect();
    let btc_low: Vec<f64> = btc_df.column("low")?.f64()?.into_no_null_iter().collect();

    let n = min_len;
    let warmup = WARMUP_BARS;
    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

    let mut trades: Vec<Trade> = Vec::new();
    let mut bar = warmup;

    while bar + 2 < n {
        // Regime entry gate
        let btc_pct = btc_atr_pct(
            &btc_close,
            &btc_high,
            &btc_low,
            REGIME_ATR_PERIOD,
            REGIME_LOOKBACK,
            bar,
        );
        if btc_pct < ATR_RANK_T {
            bar += 1;
            continue;
        }

        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in &symbols {
            let sym_str: &str = sym;
            if let Some(sd) = sym_data.get(sym_str) {
                if bar >= sd.close.len() {
                    continue;
                }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((
                    sym.as_str(),
                    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 },
                ));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(POSITION_CAP)
            .map(|(s, _)| s.to_string())
            .collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        // Try Turtle entry
        let mut entered = false;
        for sym in &top_syms {
            let sym_str: &str = sym;
            if let Some(sd) = sym_data.get(sym_str) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry_bar_next = bar + 1;
                        let n_sd = sd.close.len();

                        // Turtle ATR trailing stop
                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n_sd.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        // Seed ATR buffer from bars before entry
                        let warm_start = entry_bar_next.saturating_sub(TURTLE_ATR_PERIOD);
                        let mut atr_buf: VecDeque<f64> = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
                        for b in warm_start..entry_bar_next {
                            if b > 0 {
                                let c0 = sd.close[b.saturating_sub(1)];
                                let tr = true_range(sd.high[b], sd.low[b], c0);
                                atr_buf.push_back(tr);
                            }
                        }

                        let mut exited_turtle = false;
                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high {
                                highest_high = sd.high[b];
                            }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = true_range(sd.high[b], sd.low[b], c0);
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD {
                                atr_buf.pop_front();
                            }

                            // Turtle ATR exit (when buffer warm)
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr_sum: f64 = atr_buf.iter().sum();
                                let atr = atr_sum / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    exited_turtle = true;
                                    break;
                                }
                            }

                            // HOLD_MAX enforced independently of ATR warmup
                            if b >= entry_bar_next + HOLD_MAX {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let gross = (exit_px / entry_px - 1.0) * 100.0;
                            let net = ((exit_px * (1.0 - TAKER_FEE))
                                / (entry_px * (1.0 + TAKER_FEE))
                                - 1.0)
                                * 100.0;
                            let fee = gross - net;
                            let bars_held =
                                (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            trades.push(Trade {
                                symbol: sym.to_string(),
                                entry_bar: entry_bar_next,
                                exit_bar,
                                bars_held,
                                entry_price: entry_px,
                                exit_price: exit_px,
                                gross_pct: gross,
                                net_pct: net,
                                fee_pct: fee,
                                by_turtle: exited_turtle,
                                by_maxhold: !exited_turtle,
                            });

                            bar = exit_bar;
                            if bar >= n {
                                bar = n.saturating_sub(1);
                            }
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

    // ── ANALYSIS ─────────────────────────────────────────────────────────────────

    let n_t = trades.len();
    println!("");
    println!("T63: Per-Trade PnL Attribution Results");
    println!("{}", "=".repeat(55));

    if n_t == 0 {
        println!("\nNo trades generated. Check data and signal logic.");
        return Ok(());
    }

    let gross_sum: f64 = trades.iter().map(|t| t.gross_pct).sum();
    let fee_sum: f64 = trades.iter().map(|t| t.fee_pct).sum();
    let net_sum: f64 = trades.iter().map(|t| t.net_pct).sum();
    let full_equity: f64 = trades
        .iter()
        .fold(1.0, |eq, t| eq * (1.0 + t.net_pct / 100.0));
    let log_total: f64 = trades
        .iter()
        .map(|t| (1.0 + t.net_pct / 100.0).max(1e-12).ln())
        .sum();

    println!("\nTotal trades: {}", n_t);
    println!("Compounded equity: {:.2}x", full_equity);
    println!("Gross PnL:   {:+.1}%", gross_sum);
    println!("Fees:         {:.1}%", fee_sum);
    println!("Net PnL:      {:+.1}%", net_sum);
    if gross_sum != 0.0 {
        println!("Fees/gross:   {:.1}%", fee_sum / gross_sum.abs() * 100.0);
    }

    let winners = trades.iter().filter(|t| t.net_pct > 0.0).count();
    let losers = n_t - winners;
    println!(
        "\nWin rate:  {}/{} ({:.1}%)",
        winners,
        n_t,
        winners as f64 / n_t as f64 * 100.0
    );
    let avg_win = trades
        .iter()
        .filter(|t| t.net_pct > 0.0)
        .map(|t| t.net_pct)
        .sum::<f64>()
        / winners.max(1) as f64;
    let avg_loss = trades
        .iter()
        .filter(|t| t.net_pct <= 0.0)
        .map(|t| t.net_pct)
        .sum::<f64>()
        / losers.max(1) as f64;
    println!("Avg win:   {:+.2}%", avg_win);
    println!("Avg loss:  {:+.2}%", avg_loss);
    println!("W/L ratio: {:.2}x", avg_win / avg_loss.abs());

    // Equity concentration
    let mut sorted = trades.clone();
    sorted.sort_by(|a, b| b.net_pct.partial_cmp(&a.net_pct).unwrap());
    println!("\n--- Equity Concentration ---");
    for top in [1, 3, 5, 10, 20, 30] {
        if top > n_t {
            break;
        }
        let top_sum: f64 = sorted.iter().take(top).map(|t| t.net_pct).sum();
        let top_log: f64 = sorted
            .iter()
            .take(top)
            .map(|t| (1.0 + t.net_pct / 100.0).max(1e-12).ln())
            .sum();
        let share = if log_total.abs() > 1e-12 {
            top_log / log_total * 100.0
        } else {
            0.0
        };
        let equity_without = (log_total - top_log).exp();
        println!(
            "  Top-{:2}: {:+8.1}% additive | {:5.1}% log-share | equity w/o top = {:7.2}x",
            top, top_sum, share, equity_without
        );
    }

    // Loss concentration
    println!("\n--- Loss Concentration ---");
    for bot in [1, 3, 5, 10] {
        if bot > n_t {
            break;
        }
        let bot_sum: f64 = sorted.iter().rev().take(bot).map(|t| t.net_pct).sum();
        println!("  Bot-{:2}: {:+8.1}%", bot, bot_sum);
    }

    // Distribution
    println!("\n--- PnL Distribution ---");
    let labels = [
        ">20%", "10-20%", "5-10%", "2-5%", "0-2%", "-2-0%", "-5--2%", "-10--5%", "-20--10%",
        "<-20%",
    ];
    let mut cnt = [0usize; 10];
    let mut sum = [0.0_f64; 10];
    for t in &trades {
        let p = t.net_pct;
        let i = match p {
            p if p > 20.0 => 0,
            p if p > 10.0 => 1,
            p if p > 5.0 => 2,
            p if p > 2.0 => 3,
            p if p > 0.0 => 4,
            p if p > -2.0 => 5,
            p if p > -5.0 => 6,
            p if p > -10.0 => 7,
            p if p > -20.0 => 8,
            _ => 9,
        };
        cnt[i] += 1;
        sum[i] += p;
    }
    println!(
        "{:>10}  {:>5}  {:>10}  {:>9}",
        "Bucket", "N", "Net%", "Avg%"
    );
    for i in 0..10 {
        if cnt[i] > 0 {
            println!(
                "{:>10}  {:>5}  {:>+9.1}%  {:>+8.2}%",
                labels[i],
                cnt[i],
                sum[i],
                sum[i] / cnt[i] as f64
            );
        }
    }

    // Exit types
    let t_exit = trades.iter().filter(|t| t.by_turtle).count();
    let h_exit = trades.iter().filter(|t| t.by_maxhold).count();
    let e_exit = n_t - t_exit - h_exit;
    let t_net: f64 = trades
        .iter()
        .filter(|t| t.by_turtle)
        .map(|t| t.net_pct)
        .sum();
    let h_net: f64 = trades
        .iter()
        .filter(|t| t.by_maxhold)
        .map(|t| t.net_pct)
        .sum();
    let e_net: f64 = trades
        .iter()
        .filter(|t| !t.by_turtle && !t.by_maxhold)
        .map(|t| t.net_pct)
        .sum();
    println!("\n--- Exit Types ---");
    let fmt = |cnt, net, label| {
        let avg = if cnt > 0 { net / cnt as f64 } else { 0.0 };
        println!(
            "  {}: {:4} ({:5.1}%)  net {:+9.1}%  avg {:+.2}%/trade",
            label,
            cnt,
            cnt as f64 / n_t as f64 * 100.0,
            net,
            avg
        );
    };
    fmt(t_exit, t_net, "Turtle ATR");
    fmt(h_exit, h_net, "Max-hold");
    fmt(e_exit, e_net, "End-data");

    // Consecutive losers / losing bars in chronological order
    let mut max_trade_streak = 0usize;
    let mut cur_trade_streak = 0usize;
    let mut max_losing_bars = 0usize;
    let mut cur_losing_bars = 0usize;
    for t in &trades {
        if t.net_pct < 0.0 {
            cur_trade_streak += 1;
            max_trade_streak = max_trade_streak.max(cur_trade_streak);
            cur_losing_bars += t.bars_held;
            max_losing_bars = max_losing_bars.max(cur_losing_bars);
        } else {
            cur_trade_streak = 0;
            cur_losing_bars = 0;
        }
    }
    println!("\nMax consecutive losing trades: {}", max_trade_streak);
    println!(
        "Max consecutive attributed losing bars: {}",
        max_losing_bars
    );

    // Top / worst
    let float_str = |v: f64| -> String { format!("{:+.2}", v) };
    println!("\n--- Top 10 Trades ---");
    for (i, t) in sorted.iter().take(10).enumerate() {
        let exit = if t.by_turtle {
            "T"
        } else if t.by_maxhold {
            "H"
        } else {
            "E"
        };
        println!(
            "  {:2}. {} {}b {}% / {}% [{}]",
            i + 1,
            t.symbol,
            t.bars_held,
            float_str(t.net_pct),
            float_str(t.gross_pct),
            exit
        );
    }
    println!("\n--- Worst 10 Trades ---");
    for (i, t) in sorted.iter().rev().take(10).enumerate() {
        let exit = if t.by_turtle {
            "T"
        } else if t.by_maxhold {
            "H"
        } else {
            "E"
        };
        println!(
            "  {:2}. {} {}b {}% / {}% [{}]",
            i + 1,
            t.symbol,
            t.bars_held,
            float_str(t.net_pct),
            float_str(t.gross_pct),
            exit
        );
    }

    // Export CSV
    let csv_path = "snapshots/t63_trade_attribution.csv";
    let mut f = File::create(csv_path)?;
    writeln!(f, "symbol,entry_bar,exit_bar,bars_held,entry_price,exit_price,gross_pct,net_pct,fee_pct,by_turtle,by_maxhold")?;
    for t in &trades {
        writeln!(
            f,
            "{},{},{},{},{},{},{},{},{},{},{}",
            t.symbol,
            t.entry_bar,
            t.exit_bar,
            t.bars_held,
            t.entry_price,
            t.exit_price,
            t.gross_pct,
            t.net_pct,
            t.fee_pct,
            t.by_turtle as u32,
            t.by_maxhold as u32
        )?;
    }
    println!("\nExported: {}", csv_path);
    println!("\n=== T63 COMPLETE ===");

    Ok(())
}
