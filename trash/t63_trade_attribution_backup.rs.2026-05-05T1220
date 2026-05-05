//! T63: Per-Bar / Per-Trade PnL Attribution
//!
//! Mission: Given Turtle-only live path equity, answer:
//!   (a) Is equity concentrated in few mega-trades or distributed across many?
//!   (b) What % of gross equity comes from top-5 / top-10 / bottom-half trades?
//!   (c) Win rate, avg win, avg loss, profit factor
//!   (d) How many consecutive losing bars before 50% equity loss?
//!
//! This is the most important trust question for the strategy.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 96;
const REGIME_ATR_P: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult { return false; }
        }
        true
    } else { false }
}

fn btc_atr_pct(btc_data: &SymData, ap: usize, lb: usize, idx: usize) -> f64 {
    let warmup = ap.max(lb) + 1;
    if idx < warmup { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, ap, idx);
    let mut hist = Vec::with_capacity(lb);
    for j in (idx + 1 - lb)..=idx {
        if j >= ap {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, ap, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

#[derive(Default)]
struct TradeStats {
    total_trades: usize,
    winning_trades: usize,
    losing_trades: usize,
    gross_pnl: f64,
    gross_wins: f64,
    gross_losses: f64,
    total_fees: f64,
    all_returns: Vec<f64>,
    equity_per_trade: Vec<f64>,
    bars_in_winners: usize,
    bars_in_losers: usize,
    max_consecutive_losing_bars: usize,
    equity_by_trade_rank: Vec<f64>, // equity contribution by trade rank
}

fn run_sim_tracked(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    equity_curve: &mut Vec<f64>,
) -> TradeStats {
    let mut stats = TradeStats::default();
    let mut equity = 1.0;
    let mut bar = test_start;
    let mut trade_equity_contrib: Vec<f64> = Vec::new();
    let mut peak = 1.0;
    let mut losing_bar_streak = 0usize;
    let mut max_losing_streak = 0usize;
    let mut current_losing_streak = 0usize;

    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, REGIME_ATR_P, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
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
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry_fee = entry_px * TAKER_FEE;
                        let exit_fee: f64;
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        let mut atr_buf: VecDeque<f64> = VecDeque::new();

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
                            exit_fee = exit_px * TAKER_FEE;
                            let pct_ret = exit / (entry_px * (1.0 + TAKER_FEE)) - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            stats.total_trades += 1;
                            stats.total_fees += entry_fee + exit_fee;
                            stats.all_returns.push(pct_ret);

                            if pct_ret > 0.0 {
                                stats.winning_trades += 1;
                                stats.gross_wins += pct_ret;
                                stats.bars_in_winners += bars_held;
                                current_losing_streak = 0;
                            } else {
                                stats.losing_trades += 1;
                                stats.gross_losses += pct_ret.abs();
                                stats.bars_in_losers += bars_held;
                                current_losing_streak += bars_held;
                                if current_losing_streak > max_losing_streak {
                                    max_losing_streak = current_losing_streak;
                                }
                            }

                            let prev_equity = equity;
                            equity *= 1.0 + pct_ret;
                            trade_equity_contrib.push(equity / prev_equity - 1.0);
                            equity_curve.push(equity);

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
            equity_curve.push(equity);
            bar += 1;
        }
    }

    stats.max_consecutive_losing_bars = max_losing_streak;

    // Compute equity contribution per trade rank
    let mut with_ranking: Vec<(usize, f64)> = trade_equity_contrib.into_iter().enumerate().collect();
    with_ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let total_contrib: f64 = with_ranking.iter().map(|(_, c)| c.abs()).sum();
    let mut running_pct = 0.0;
    stats.equity_by_trade_rank = with_ranking.iter().map(|(rank, &c)| {
        running_pct += c.abs() / total_contrib;
        running_pct
    }).collect();

    stats
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T63: Per-Trade PnL Attribution ===\");
    println!();

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter().copied()).collect::<std::collections::HashSet<_>>();
    for sym in all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let mut all_trade_stats: Vec<TradeStats> = Vec::new();
    let mut all_equity_curves: Vec<Vec<f64>> = Vec::new();
    let mut universe_names: Vec<String> = Vec::new();
    let mut window_ids: Vec<String> = Vec::new();

    let min_len = sym_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;

    let mut detail_file = File::create("snapshots/t63_trade_detail.csv")?;
    writeln!(detail_file, "universe,window,trade_num,return_pct,bars_held,winners_after,equity_cumulative")?;

    let mut agg_win_returns: Vec<f64> = Vec::new();
    let mut agg_loss_returns: Vec<f64> = Vec::new();
    let mut agg_all_returns: Vec<f64> = Vec::new();
    let mut total_trades = 0usize;
    let mut total_wins = 0usize;
    let mut total_loss = 0usize;
    let mut total_fees = 0.0;
    let mut gross_wins_total = 0.0;
    let mut gross_losses_total = 0.0;
    let mut total_bars_winners = 0usize;
    let mut total_bars_losers = 0usize;
    let mut max_streak = 0usize;
    let mut max_eq_from_one_trade = 0.0f64;
    let mut all_trade_eq_contribs: Vec<f64> = Vec::new();

    for (uni_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;

            let mut equity_curve = Vec::new();
            let stats = run_sim_tracked(&sym_data, &syms, test_start, end, &mut equity_curve);
            all_equity_curves.push(equity_curve);
            universe_names.push(uni_name.to_string());
            window_ids.push(format!("{}_{}", uni_name, w));

            if stats.total_trades > 0 {
                for (ti, &ret) in stats.all_returns.iter().enumerate() {
                    writeln!(detail_file, "{},{},{},{:.4f},{},{},{:.6}",
                        uni_name, w, ti, ret,
                        if ret > 0.0 { stats.winning_trades } else { 0 },
                        1.0 + stats.all_returns[..=ti].iter().map(|&x| x).sum::<f64>())?;
                }
            }

            total_trades += stats.total_trades;
            total_wins += stats.winning_trades;
            total_loss += stats.losing_trades;
            total_fees += stats.total_fees;
            gross_wins_total += stats.gross_wins;
            gross_losses_total += stats.gross_losses;
            total_bars_winners += stats.bars_in_winners;
            total_bars_losers += stats.bars_in_losers;
            if stats.max_consecutive_losing_bars > max_streak {
                max_streak = stats.max_consecutive_losing_bars;
            }
            agg_win_returns.extend(stats.all_returns.iter().filter(|&&r| r > 0.0).copied());
            agg_loss_returns.extend(stats.all_returns.iter().filter(|&&r| r <= 0.0).copied());
            agg_all_returns.extend(stats.all_returns.iter().copied());
            all_trade_eq_contribs.extend(stats.equity_by_trade_rank.iter().copied());
            all_trade_stats.push(stats);
        }
    }

    // ----- AGGREGATE STATS                                                                                                                                                    
    let win_rate = total_trades as f64 / (total_wins.max(1) as f64);
    let avg_win = if total_wins > 0 { gross_wins_total / total_wins as f64 } else { 0.0 };
    let avg_loss = if total_loss > 0 { gross_losses_total / total_loss as f64 } else { 0.0 };
    let profit_factor = if gross_losses_total > 0.0 { gross_wins_total / gross_losses_total } else { 0.0 };
    let avg_return = if total_trades > 0 { agg_all_returns.iter().sum::<f64>() / total_trades as f64 } else { 0.0 };
    let avg_bars_winner = if total_wins > 0 { total_bars_winners as f64 / total_wins as f64 } else { 0.0 };
    let avg_bars_loser = if total_loss > 0 { total_bars_losers as f64 / total_loss as f64 } else { 0.0 };

    // Equity concentration: sort trades by return contribution
    let mut sorted_contribs = all_trade_eq_contribs.clone();
    sorted_contribs.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let top5_pct = sorted_contribs.iter().take(5).sum::<f64>() * 100.0;
    let top10_pct = sorted_contribs.iter().take(10).sum::<f64>() * 100.0;
    let top20_pct = sorted_contribs.iter().take((total_trades / 5).max(1)).sum::<f64>() * 100.0;
    let bottom50_pct: f64 = sorted_contribs.iter().rev().take(total_trades / 2).sum::<f64>() * 100.0;

    // Cumulative equity curve
    let portfolio_equity: Vec<f64> = {
        let n = all_equity_curves[0].len();
        let mut port = vec![1.0; n];
        for curve in &all_equity_curves {
            for (i, &v) in curve.iter().enumerate() {
                if i < n {
                    port[i] *= v;
                }
            }
        }
        port
    };

    let final_equity = portfolio_equity.last().copied().unwrap_or(1.0);
    let peak = portfolio_equity.iter().fold(1.0f64, |a, &b| a.max(b));
    let max_dd = {
        let mut mdd = 0.0;
        let mut p = 1.0;
        for &v in &portfolio_equity {
            if v > p { p = v; }
            let dd = 1.0 - v / p;
            if dd > mdd { mdd = dd; }
        }
        mdd * 100.0
    };

    // Annualised Sharpe from daily returns
    let daily_rets: Vec<f64> = portfolio_equity.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean_ret = if !daily_rets.is_empty() { daily_rets.iter().sum::<f64>() / daily_rets.len() as f64 } else { 0.0 };
    let var_ret = if !daily_rets.is_empty() { daily_rets.iter().map(|x| (x - mean_ret).powi(2)).sum::<f64>() / daily_rets.len() as f64 } else { 0.0 };
    let sharpe = if var_ret > 0.0 { (mean_ret / var_ret.sqrt()) * (365.0_f64).sqrt() } else { 0.0 };

    // ----- PRINT REPORT                                                                                                                                                                
    println!("=== T63: Per-Trade PnL Attribution ===");
    println!();
    println!("OVERALL ({} universes x {} windows, {} trades)", UNIVERSES.len(), windows, total_trades);
    println!("{:<30} {:>12.3}", "Portfolio Final Equity:", final_equity);
    println!("{:<30} {:>12.3}", "Portfolio Sharpe:", sharpe);
    println!("{:<30} {:>12.2}%", "Max Drawdown:", max_dd);
    println!();
    println!("TRADE DISTRIBUTION");
    println!("{:<30} {:>10} ({:.1}% of {} total)",
        "Total Trades:", total_trades, 100.0, total_trades);
    println!("{:<30} {:>10} ({:.1}%)", "Winning trades:", total_wins, total_wins as f64 / total_trades.max(1) as f64 * 100.0);
    println!("{:<30} {:>10} ({:.1}%)", "Losing trades:", total_loss, total_loss as f64 / total_trades.max(1) as f64 * 100.0);
    println!("{:<30} {:>10.4}%", "Avg Return per trade:", avg_return * 100.0);
    println!("{:<30} {:>10.4}%", "Avg Winning trade:", avg_win * 100.0);
    println!("{:<30} {:>10.4}%", "Avg Losing trade:", -(avg_loss * 100.0));
    println!("{:<30} {:>10.4}", "Profit Factor:", profit_factor);
    println!("{:<30} {:>10.1} bars", "Avg bars in winners:", avg_bars_winner);
    println!("{:<30} {:>10.1} bars", "Avg bars in losers:", avg_bars_loser);
    println!("{:<30} {:>10.4}%", "Total fees paid:", total_fees * 100.0);
    println!();
    println!("EQUITY CONCENTRATION");
    println!("{:<30} {:>10.2}%", "Top-5 trades:", top5_pct);
    println!("{:<30} {:>10.2}%", "Top-10 trades:", top10_pct);
    println!("{:<30} {:>10.2}%", "Top-20% of trades:", top20_pct);
    println!("{:<30} {:>10.2}%", "Bottom-50% of trades:", bottom50_pct);
    println!();
    println!("RISK METRICS");
    println!("{:<30} {:>10} bars", "Max consecutive losing bars:", max_streak);
    println!();

    // Percentile analysis
    let mut sorted_returns = agg_all_returns.clone();
    sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_95 = sorted_returns[(sorted_returns.len() as f64 * 0.95) as usize].min(sorted_returns.len()-1)];
    let pct_75 = sorted_returns[(sorted_returns.len() as f64 * 0.75) as usize].min(sorted_returns.len()-1)];
    let pct_50 = sorted_returns[(sorted_returns.len() as f64 * 0.50) as usize].min(sorted_returns.len()-1)];
    let pct_25 = sorted_returns[(sorted_returns.len() as f64 * 0.25) as usize].min(sorted_returns.len()-1)];
    let pct_5  = sorted_returns[(sorted_returns.len() as f64 * 0.05) as usize].min(sorted_returns.len()-1)];
    println!("RETURN DISTRIBUTION");
    println!("{:<30} {:>10.2}%", "P95 (best):", pct_95 * 100.0);
    println!("{:<30} {:>10.2}%", "P75:", pct_75 * 100.0);
    println!("{:<30} {:>10.2}%", "P50 (median):", pct_50 * 100.0);
    println!("{:<30} {:>10.2}%", "P25:", pct_25 * 100.0);
    println!("{:<30} {:>10.2}%", "P5 (worst):", pct_5 * 100.0);

    // ----- WRITE EQUITY CURVE                                                                                                                                                 
    let mut eq_file = File::create("snapshots/t63_portfolio_equity.csv")?;
    writeln!(eq_file, "step,equity")?;
    for (i, &eq) in portfolio_equity.iter().enumerate() {
        writeln!(eq_file, "{},{:.6}", i, eq)?;
    }

    // ----- WRITE MARKDOWN REPORT                                                                                                                                        
    let mut md_file = File::create("snapshots/t63_report.md")?;
    writeln!(md_file, "# T63: Per-Trade PnL Attribution Report")?;
    writeln!(md_file)?;
    writeln!(md_file, "**Date:** 2026-05-05")?;
    writeln!(md_file, "**Scope:** {} universes x {} WF windows, {} trades total", UNIVERSES.len(), windows, total_trades)?;
    writeln!(md_file)?;
    writeln!(md_file, "## Portfolio Summary")?;
    writeln!(md_file, "| Metric | Value |")?;
    writeln!(md_file, "|--------|-------|")?;
    writeln!(md_file, "| Final Equity | {:.3f}x |", final_equity)?;
    writeln!(md_file, "| Annualised Sharpe | {:.3f} |", sharpe)?;
    writeln!(md_file, "| Max Drawdown | {:.2f}% |", max_dd)?;
    writeln!(md_file, "| Total Trades | {} |", total_trades)?;
    writeln!(md_file, "| Total Fees | {:.4f}% |", total_fees * 100.0)?;
    writeln!(md_file, "## Trade Distribution")?;
    writeln!(md_file, "| Metric | Value |")?;
    writeln!(md_file, "|--------|-------|")?;
    writeln!(md_file, "| Win Rate | {:.1}% |", total_wins as f64 / total_trades.max(1) as f64 * 100.0)?;
    writeln!(md_file, "| Avg Win | {:.3f}% |", avg_win * 100.0)?;
    writeln!(md_file, "| Avg Loss | {:.3f}% |", -(avg_loss * 100.0))?;
    writeln!(md_file, "| Profit Factor | {:.3f} |", profit_factor)?;
    writeln!(md_file, "| Avg Bars (winners) | {:.1f} |", avg_bars_winner)?;
    writeln!(md_file, "| Avg Bars (losers) | {:.1f} |", avg_bars_loser)?;
    writeln!(md_file, "| Median Return | {:.3f}% |", pct_50 * 100.0)?;
    writeln!(md_file, "## Equity Concentration")?;
    writeln!(md_file, "| Top-N Trades | Equity % |")?;
    writeln!(md_file, "|-----------|--------|")?;
    writeln!(md_file, "| Top-5 | {:.1f}% |", top5_pct)?;
    writeln!(md_file, "| Top-10 | {:.1f}% |", top10_pct)?;
    writeln!(md_file, "| Top-20% | {:.1f}% |", top20_pct)?;
    writeln!(md_file, "| Bottom-50% | {:.1f}% |", bottom50_pct)?;
    writeln!(md_file, "## Risk")?;
    writeln!(md_file, "| Metric | Value |")?;
    writeln!(md_file, "|--------|-------|")?;
    writeln!(md_file, "| Max Consecutive Losing Bars | {} |", max_streak)?;
    writeln!(md_file, "| P5 Return | {:.3f}% |", pct_5 * 100.0)?;
    writeln!(md_file, "| P95 Return | {:.3f}% |", pct_95 * 100.0)?;
    let verdict = if top5_pct > 50.0 {
        "**WARNING FRAGILE:** Top-5 trades account for >50% of equity. Strategy depends on mega-trends."
    } else if top10_pct > 70.0 {
        "**WARNING CONCENTRATED:** Top-10 trades account for >70% of equity. Moderately fragile."
    } else {
        "**OK ROBUST:** Equity is distributed across many trades. Strategy is resilient."
    };
    writeln!(md_file)?;
    writeln!(md_file, "## Verdict")?;
    writeln!(md_file, "{}", verdict)?;

    println!();
    println!("{}", verdict);

    println!();
    println!("Files written:");
    println!("  snapshots/t63_portfolio_equity.csv");
    println!("  snapshots/t63_trade_detail.csv");
    println!("  snapshots/t63_report.md");

    Ok(())
}
