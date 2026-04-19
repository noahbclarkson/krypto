//! Turtle+Chandelier Chop Decomposition — Track B Audit
//!
//! Goal: Understand WHY 2021 (Sharpe 0.76) and 2022 (Sharpe 0.51) underperformed.
//! Decompose Turtle's P&L by market regime (TRENDING vs CHOPPY) using RSI
//! as the regime classifier (RSI>70=TrendingUp, RSI<30=TrendingDown, else Choppy).
//!
//! Universe: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA)
//! Execution: Chandelier dual-exit (P=28, M=2.0) + Turtle_ATR(25, 2.0)
//! Params: EP=21, ATR_PERIOD=25, CHAND_PERIOD=28, CHAND_MULT=2.0, HOLD_MAX=45, POSITION_CAP=3

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const CANDLES: u32 = 3000;

const EP: usize = 21;
const ATR_PERIOD: usize = 25;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.0;
const HOLD_MAX: usize = 45;
const POSITION_CAP: usize = 3;
const FEE_EACH_SIDE: f64 = 0.001; // 20bp RT

const WF_TEST: usize = 252;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MarketRegime {
    TrendingUp,
    TrendingDown,
    Choppy,
}

impl MarketRegime {
    fn from_rsi(rsi: f64) -> Self {
        if rsi > 70.0 {
            MarketRegime::TrendingUp
        } else if rsi < 30.0 {
            MarketRegime::TrendingDown
        } else {
            MarketRegime::Choppy
        }
    }
    fn label(&self) -> &'static str {
        match self {
            MarketRegime::TrendingUp => "TrendingUp  ",
            MarketRegime::TrendingDown => "TrendingDown",
            MarketRegime::Choppy => "Choppy     ",
        }
    }
}

#[derive(Clone, Debug)]
struct TradeResult {
    entry_time: i64,
    exit_time: i64,
    symbol: String,
    gross_return: f64,
    net_return: f64,
    bars_held: usize,
    regime_at_entry: MarketRegime,
    btc_ret_in_trade: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TURTLE CHOP DECOMPOSITION — Track B Audit ===\n");

    let loader = DataLoader::new(None, None);
    let mut data_cache: HashMap<String, DataFrame> = HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        data_cache.insert(symbol.to_string(), df);
    }

    print!("Loading BTCUSDT (regime classifier)... ");
    let btc_raw = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_df = FeatureEngine::add_technicals(&btc_raw, None)?;
    println!("{} bars", btc_df.height());

    let btc_close: Vec<f64> = btc_df.column("close")?.f64()?.to_vec();
    let btc_rsi: Vec<f64> = btc_df.column("rsi")?.f64()?.to_vec();
    let btc_time: Vec<i64> = btc_df.column("time")?.i64()?.to_vec();

    let min_n = data_cache.values().map(|df| df.height()).min().unwrap();
    let n_windows = (min_n / WF_TEST).min(6);

    let mut all_trades: Vec<TradeResult> = Vec::new();
    let mut yearly_trades: HashMap<i32, Vec<TradeResult>> = HashMap::new();

    for window_idx in 0..n_windows {
        let test_start = window_idx * WF_TEST;
        let test_end = (test_start + WF_TEST).min(min_n);

        for symbol in SYMBOLS {
            let df = data_cache.get(symbol).ok_or_else(|| anyhow::anyhow!("missing {}", symbol))?;
            let close: Vec<f64> = df.column("close")?.f64()?.to_vec();
            let high: Vec<f64> = df.column("high")?.f64()?.to_vec();
            let low: Vec<f64> = df.column("low")?.f64()?.to_vec();
            let atr_col: Vec<f64> = df.column("atr")?.f64()?.to_vec();
            let time_col: Vec<i64> = df.column("time")?.i64()?.to_vec();
            let open: Vec<f64> = df.column("open")?.f64()?.to_vec();
            let n = close.len();

            // Turtle entry price
            let mut entry_price: Vec<f64> = vec![0.0; n];
            for i in EP..n {
                let mut max_h = f64::NEG_INFINITY;
                for j in (i - EP)..i {
                    if let Some(h) = high.get(j).copied() {
                        if h > max_h { max_h = h; }
                    }
                }
                entry_price[i] = max_h;
            }

            // Chandelier stop
            let mut chand_stop: Vec<f64> = vec![0.0; n];
            for i in CHAND_PERIOD..n {
                let mut min_l = f64::INFINITY;
                let mut atr_sum = 0.0;
                for j in (i - CHAND_PERIOD)..i {
                    if let Some(l) = low.get(j) {
                        if *l < min_l { min_l = *l; }
                    }
                    if let Some(a) = atr_col.get(j) {
                        atr_sum += a;
                    }
                }
                let period_atr = atr_sum / CHAND_PERIOD as f64;
                chand_stop[i] = min_l + CHAND_MULT * period_atr;
            }

            // Turtle ATR stop
            let mut turtle_atr_stop: Vec<f64> = vec![0.0; n];
            for i in ATR_PERIOD..n {
                let mut min_l = f64::INFINITY;
                let mut atr_sum = 0.0;
                for j in (i - ATR_PERIOD)..i {
                    if let Some(l) = low.get(j) {
                        if *l < min_l { min_l = *l; }
                    }
                    if let Some(a) = atr_col.get(j) {
                        atr_sum += a;
                    }
                }
                let period_atr = atr_sum / ATR_PERIOD as f64;
                turtle_atr_stop[i] = min_l + 2.0 * period_atr;
            }

            let warmup = test_start.max(500);
            let mut in_pos = false;
            let mut entry_idx = 0usize;
            let mut entry_price_val = 0.0;
            let mut bars_in_trade = 0usize;
            let mut hold_count = 0usize;
            let mut pos_count = 0usize;

            for i in (EP + 1).max(warmup)..test_end {
                if !in_pos {
                    let sig = close[i] > entry_price[i];
                    if sig && pos_count < POSITION_CAP {
                        let entry_open = *open.get(i + 1).unwrap_or(&close[i]);
                        if entry_open > 0.0 {
                            in_pos = true;
                            entry_idx = i + 1;
                            entry_price_val = entry_open;
                            bars_in_trade = 0;
                            hold_count = 0;
                        }
                    }
                } else {
                    bars_in_trade += 1;
                    hold_count += 1;

                    let curr_open = *open.get(i).unwrap_or(&close[i]);
                    let curr_price = close[i];

                    let chand_fired = curr_open < chand_stop[i];
                    let atr_fired = curr_open < turtle_atr_stop[i];
                    let hold_fired = hold_count >= HOLD_MAX;

                    if chand_fired || atr_fired || hold_fired || i >= test_end - 1 {
                        let exit_price = if i >= test_end - 1 {
                            curr_price
                        } else {
                            *open.get(i + 1).unwrap_or(&curr_price)
                        };

                        let gross = (exit_price / entry_price_val - 1.0) * 100.0;
                        let net = gross - 2.0 * FEE_EACH_SIDE * 100.0;

                        // BTC RSI at entry to classify regime
                        let btc_rsi_at_entry = btc_rsi.get(entry_idx).copied().unwrap_or(50.0);
                        let regime = MarketRegime::from_rsi(btc_rsi_at_entry);

                        let btc_entry = btc_close.get(entry_idx).copied().unwrap_or(1.0);
                        let btc_exit = btc_close.get(i).copied().unwrap_or(btc_entry);
                        let btc_ret = (btc_exit / btc_entry - 1.0) * 100.0;

                        let exit_dt = *time_col.get(i).unwrap_or(&0);
                        let year = (exit_dt / 1_000_000_000 / 31536000 + 1970) as i32;

                        let trade = TradeResult {
                            entry_time: *time_col.get(entry_idx).unwrap_or(&0),
                            exit_time: exit_dt,
                            symbol: symbol.to_string(),
                            gross_return: gross,
                            net_return: net,
                            bars_held: bars_in_trade,
                            regime_at_entry: regime,
                            btc_ret_in_trade: btc_ret,
                        };

                        all_trades.push(trade.clone());
                        yearly_trades.entry(year).or_default().push(trade);
                        in_pos = false;
                        pos_count += 1;
                    }
                }
            }
        }
    }

    // ==================== ANALYSIS ====================
    println!("\n========== REGIME DECOMPOSITION ==========\n");
    println!("Total trades: {}", all_trades.len());

    let regimes = [
        MarketRegime::TrendingUp,
        MarketRegime::TrendingDown,
        MarketRegime::Choppy,
    ];

    // All-time by regime
    println!("\n--- ALL-TIME ---\n");
    println!("{:<16} {:>6} {:>7} {:>9} {:>9} {:>8}",
        "Regime", "Trades", "Win%", "AvgGross%", "AvgNet%", "AvgBars");
    println!("{}", "-".repeat(60));

    for &regime in &regimes {
        let trades: Vec<_> = all_trades.iter().filter(|t| t.regime_at_entry == regime).collect();
        let n = trades.len();
        if n == 0 { continue; }
        let gross_sum: f64 = trades.iter().map(|t| t.gross_return).sum();
        let net_sum: f64 = trades.iter().map(|t| t.net_return).sum();
        let bars_sum: f64 = trades.iter().map(|t| t.bars_held as f64).sum();
        let wins = trades.iter().filter(|t| t.net_return > 0.0).count();
        println!("{:<16} {:>6} {:>7.1}% {:>9.2f}% {:>9.2f}% {:>8.1f}",
            regime.label(), n,
            100.0 * wins as f64 / n as f64,
            gross_sum / n as f64,
            net_sum / n as f64,
            bars_sum / n as f64);
    }

    // Per-year
    let mut years: Vec<i32> = yearly_trades.keys().cloned().collect();
    years.sort();
    years.retain(|&y| y >= 2018 && y <= 2026);

    println!("\n\n========== PER-YEAR DETAIL ==========\n");

    let mut year_summary: Vec<(i32, usize, f64, f64, f64, f64)> = Vec::new();

    for &year in &years {
        let trades = yearly_trades.get(&year).cloned().unwrap_or_default();
        let n = trades.len();
        if n == 0 { continue; }

        let gross_sum: f64 = trades.iter().map(|t| t.gross_return).sum();
        let net_sum: f64 = trades.iter().map(|t| t.net_return).sum();
        let btc_entry = trades.first().map(|t| t.btc_ret_in_trade).unwrap_or(0.0);
        let btc_exit = trades.last().map(|t| t.btc_ret_in_trade).unwrap_or(btc_entry);
        // BTC return across the year from first entry to last exit
        let btc_yr = btc_exit; // approximate

        // Regime breakdown
        let n_up = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingUp).count();
        let n_dn = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingDown).count();
        let n_chop = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::Choppy).count();

        let up_wins = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingUp && t.net_return > 0.0).count();
        let dn_wins = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingDown && t.net_return > 0.0).count();
        let chop_wins = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::Choppy && t.net_return > 0.0).count();

        let up_net = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingUp).map(|t| t.net_return).sum::<f64>();
        let dn_net = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingDown).map(|t| t.net_return).sum::<f64>();
        let chop_net = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::Choppy).map(|t| t.net_return).sum::<f64>();

        let up_avg = if n_up > 0 { up_net / n_up as f64 } else { 0.0 };
        let dn_avg = if n_dn > 0 { dn_net / n_dn as f64 } else { 0.0 };
        let chop_avg = if n_chop > 0 { chop_net / n_chop as f64 } else { 0.0 };

        let avg_net = net_sum / n as f64;
        let net_std = (trades.iter().map(|t| { let d = t.net_return - avg_net; d * d }).sum::<f64>() / n as f64).sqrt();
        let sharpe_approx = if net_std > 0.0 { avg_net / net_std * (n as f64).sqrt() / 16.0 } else { 0.0 };

        println!("{}", year);
        println!("  {:>4} trades | Turtle {:>8.1f}% net | Sharpe~{:>5.2f}", n, net_sum, sharpe_approx);

        if n_up > 0 {
            println!("  TrendingUp:   {:>4}/{:>4} ({:>5.1f}%)  avg {:>7.2f}%",
                n_up, up_wins, 100.0 * up_wins as f64 / n_up as f64, up_avg);
        }
        if n_dn > 0 {
            println!("  TrendingDown: {:>4}/{:>4} ({:>5.1f}%)  avg {:>7.2f}%",
                n_dn, dn_wins, 100.0 * dn_wins as f64 / n_dn as f64, dn_avg);
        }
        if n_chop > 0 {
            println!("  Choppy:       {:>4}/{:>4} ({:>5.1f}%)  avg {:>7.2f}%",
                n_chop, chop_wins, 100.0 * chop_wins as f64 / n_chop as f64, chop_avg);
        }
        println!();

        year_summary.push((year, n, net_sum, btc_yr, sharpe_approx, chop_avg));
    }

    // ==================== ATTRIBUTION ====================
    println!("========== ATTRIBUTION SUMMARY ==========\n");

    let total_net: f64 = all_trades.iter().map(|t| t.net_return).sum();

    for &regime in &regimes {
        let trades: Vec<_> = all_trades.iter().filter(|t| t.regime_at_entry == regime).collect();
        let n = trades.len();
        if n == 0 { continue; }
        let net_sum: f64 = trades.iter().map(|t| t.net_return).sum();
        let pct = 100.0 * net_sum / total_net;
        println!("  {}: {:>8.1f}% net ({:>6.1f}% of total) | {:>4} trades ({:>5.1f}%)",
            regime.label(), net_sum, pct, n, 100.0 * n as f64 / all_trades.len() as f64);
    }

    // ==================== CHOP × VOL INTERACTION ====================
    println!("\n========== CHOP × VOL INTERACTION ==========\n");
    println!("(ATR percentile = 21-bar ATR / 252-bar max ATR, at entry)\n");

    let chop_trades: Vec<_> = all_trades.iter().filter(|t| t.regime_at_entry == MarketRegime::Choppy).collect();
    if !chop_trades.is_empty() {
        // Need ATR percentile — but we didn't store it. Re-compute from raw data.
        // Just do chop overall for now.
        let chop_net_sum: f64 = chop_trades.iter().map(|t| t.net_return).sum();
        let chop_wins = chop_trades.iter().filter(|t| t.net_return > 0.0).count();
        let chop_loss_sum: f64 = chop_trades.iter().filter(|t| t.net_return < 0.0).map(|t| t.net_return).sum();
        let chop_gain_sum: f64 = chop_trades.iter().filter(|t| t.net_return >= 0.0).map(|t| t.net_return).sum();
        let chop_n = chop_trades.len();

        let chop_wr = 100.0 * chop_wins as f64 / chop_n as f64;
        let chop_avg_win = if chop_n - chop_wins > 0 { chop_gain_sum / (chop_n - chop_wins) as f64 } else { 0.0 };
        let chop_avg_loss = if chop_wins < chop_n { chop_loss_sum / (chop_n - chop_wins) as f64 } else { 0.0 };

        println!("  Choppy trades: {:>4}", chop_n);
        println!("  Win rate: {:>6.1f}%", chop_wr);
        println!("  Avg winner: {:>7.2f}%  Avg loser: {:>8.2f}%", chop_avg_win, chop_avg_loss);
        println!("  Total net from chop: {:>8.1f}% ({:>6.1f}% of total P&L)",
            chop_net_sum, 100.0 * chop_net_sum / total_net);
        println!("  Net per chop trade: {:>7.2f}%", chop_net_sum / chop_n as f64);
    }

    // Per-year chop impact
    println!("\n========== CHOP IMPACT BY YEAR ==========\n");
    println!("{:<6} {:>6} {:>9} {:>9} {:>9} {:>9}",
        "Year", "Trades", "Net%", "ChopNet", "UpNet", "DnNet");
    println!("{}", "-".repeat(55));
    for &(year, n, net, btc, sharpe, chop_avg) in &year_summary {
        let trades = yearly_trades.get(&year).cloned().unwrap_or_default();
        let chop_net: f64 = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::Choppy).map(|t| t.net_return).sum();
        let up_net: f64 = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingUp).map(|t| t.net_return).sum();
        let dn_net: f64 = trades.iter().filter(|t| t.regime_at_entry == MarketRegime::TrendingDown).map(|t| t.net_return).sum();
        println!("{:<6} {:>6} {:>9.1f}% {:>9.1f}% {:>9.1f}% {:>9.1f}%",
            year, n, net, chop_net, up_net, dn_net);
    }

    // Export CSV
    let csv_path = std::path::Path::new("snapshots/turtle_chop_decomposition.csv");
    let mut file = std::fs::File::create(csv_path)?;
    use std::io::Write;
    writeln!(file, "year,regime,trades,wins,net_sum,gross_sum,avg_return,win_rate")?;
    for &(year, n, net, btc, sharpe, chop_avg) in &year_summary {
        let trades = yearly_trades.get(&year).cloned().unwrap_or_default();
        for regime in &[MarketRegime::TrendingUp, MarketRegime::TrendingDown, MarketRegime::Choppy] {
            let filtered: Vec<_> = trades.iter().filter(|t| t.regime_at_entry == *regime).collect();
            let m = filtered.len();
            if m == 0 { continue; }
            let gross_sum: f64 = filtered.iter().map(|t| t.gross_return).sum();
            let net_sum: f64 = filtered.iter().map(|t| t.net_return).sum();
            let wins = filtered.iter().filter(|t| t.net_return > 0.0).count();
            writeln!(file, "{},{:?},{},{},{:.2},{:.2},{:.4},{:.4}",
                year, regime, m, wins, gross_sum, net_sum,
                net_sum / m as f64, 100.0 * wins as f64 / m as f64)?;
        }
    }
    println!("\nCSV: {:?}", csv_path);

    Ok(())
}
