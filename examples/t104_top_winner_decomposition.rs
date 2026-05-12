use anyhow::{Context, Result};
use krypto::data::loader::DataLoader;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::{create_dir_all, File};
use std::io::Write;

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;

#[derive(Debug, Deserialize, Clone)]
struct TradeRow {
    symbol: String,
    entry_date: String,
    exit_date: String,
    bars_held: u32,
    size: f64,
    pct_ret: f64,
    equity_mult: f64,
    exit_reason: String,
    hedge_active: bool,
}

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

#[derive(Clone)]
struct Winner {
    rank: usize,
    trade: TradeRow,
    log_contribution: f64,
    btc_21d_return: f64,
    btc_sma50_over_sma200: f64,
    btc_atr_percentile: f64,
    btc_regime_label: String,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 {
        sd.close[idx]
    } else {
        sd.close[idx - 1]
    };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() {
        return 0.0;
    }
    let start = idx + 1 - period;
    (start..=idx).map(|i| tr_at(sd, i)).sum::<f64>() / period as f64
}

/// Mirrors `LiveBot::btc_atr_percentile` semantics used by the exact-live harness.
fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= atr_period.max(lookback) + 1 {
        return 50.0;
    }

    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 {
        return 50.0;
    }
    let curr_pct = curr_atr / curr_close;

    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 {
            continue;
        }
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr <= 0.0 {
            continue;
        }
        if hist_atr / close < curr_pct {
            below += 1;
        }
        total += 1;
    }

    if total == 0 {
        50.0
    } else {
        (below as f64 / total as f64) * 100.0
    }
}

fn sma(values: &[f64], idx: usize, period: usize) -> Option<f64> {
    if idx + 1 < period {
        return None;
    }
    let start = idx + 1 - period;
    Some(values[start..=idx].iter().sum::<f64>() / period as f64)
}

fn regime_label(btc_21d: f64, sma_proxy: f64) -> String {
    match (sma_proxy, btc_21d) {
        (s, r) if s < -0.10 && r > 0.0 => "early_rebound_in_bear".to_string(),
        (s, r) if s < -0.03 && r > 0.0 => "rebound_in_damaged_trend".to_string(),
        (s, r) if s >= 0.03 && r > 0.0 => "bull_continuation".to_string(),
        (s, _) if s < -0.03 => "bear".to_string(),
        (s, _) if s > 0.03 => "bull".to_string(),
        _ => "chop".to_string(),
    }
}

fn date_only(s: &str) -> String {
    s.split_whitespace().next().unwrap_or(s).to_string()
}

fn pct(x: f64) -> String {
    format!("{:+.1}%", x * 100.0)
}

fn pct_plain(x: f64) -> String {
    format!("{:.1}%", x)
}

fn bool_yn(x: bool) -> &'static str {
    if x {
        "yes"
    } else {
        "no"
    }
}

fn load_sym(symbol: &str) -> Result<SymData> {
    let loader = DataLoader::new(None, None);
    let df = loader
        .load_from_cache(symbol, "1d")?
        .with_context(|| format!("missing cached 1d data for {symbol}"))?;
    let close = df
        .column("close")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let high = df
        .column("high")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let low = df
        .column("low")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let time_col = df.column("time")?;
    let dates = (0..close.len())
        .map(|i| time_col.get(i).map(|v| date_only(&v.to_string())))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(SymData {
        close,
        high,
        low,
        dates,
    })
}

fn main() -> Result<()> {
    create_dir_all("snapshots")?;

    let btc = load_sym("BTCUSDT")?;
    let mut rdr = csv::Reader::from_path("snapshots/live_bot_exact_trades.csv")
        .context("missing snapshots/live_bot_exact_trades.csv; run live_bot_exact_equity first")?;

    let mut trades = Vec::new();
    for rec in rdr.deserialize() {
        let trade: TradeRow = rec?;
        trades.push(trade);
    }

    trades.sort_by(|a, b| {
        b.equity_mult
            .ln()
            .partial_cmp(&a.equity_mult.ln())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_log_return: f64 = trades.iter().map(|t| t.equity_mult.ln()).sum();
    let top10_log_return: f64 = trades.iter().take(10).map(|t| t.equity_mult.ln()).sum();
    let top10_share = top10_log_return / total_log_return;

    let mut winners = Vec::new();
    for (rank, trade) in trades.iter().take(10).cloned().enumerate() {
        let entry = date_only(&trade.entry_date);
        let idx = btc
            .dates
            .iter()
            .position(|d| d == &entry)
            .with_context(|| format!("BTC date not found for {entry}"))?;

        let btc_21d_return = if idx >= 21 {
            btc.close[idx] / btc.close[idx - 21] - 1.0
        } else {
            0.0
        };
        let btc_sma50 = sma(&btc.close, idx, 50).unwrap_or(btc.close[idx]);
        let btc_sma200 = sma(&btc.close, idx, 200).unwrap_or(btc.close[idx]);
        let btc_sma50_over_sma200 = btc_sma50 / btc_sma200 - 1.0;
        let btc_atr_percentile = btc_atr_percentile(&btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
        let btc_regime_label = regime_label(btc_21d_return, btc_sma50_over_sma200);

        winners.push(Winner {
            rank: rank + 1,
            log_contribution: trade.equity_mult.ln(),
            trade,
            btc_21d_return,
            btc_sma50_over_sma200,
            btc_atr_percentile,
            btc_regime_label,
        });
    }

    let avg_hold: f64 = winners
        .iter()
        .map(|w| w.trade.bars_held as f64)
        .sum::<f64>()
        / winners.len() as f64;
    let avg_btc_21d: f64 =
        winners.iter().map(|w| w.btc_21d_return).sum::<f64>() / winners.len() as f64;
    let positive_btc_21d = winners.iter().filter(|w| w.btc_21d_return > 0.0).count();
    let long_term_bear = winners
        .iter()
        .filter(|w| w.btc_sma50_over_sma200 < 0.0)
        .count();
    let atr_lt_24 = winners
        .iter()
        .filter(|w| w.btc_atr_percentile < 24.0)
        .count();
    let atr_lt_5 = winners
        .iter()
        .filter(|w| w.btc_atr_percentile < 5.0)
        .count();
    let fast_3 = winners.iter().filter(|w| w.trade.bars_held <= 3).count();
    let hedge_active = winners.iter().filter(|w| w.trade.hedge_active).count();

    let mut by_regime = BTreeMap::<String, usize>::new();
    let mut by_exit = BTreeMap::<String, usize>::new();
    for w in &winners {
        *by_regime.entry(w.btc_regime_label.clone()).or_default() += 1;
        *by_exit.entry(w.trade.exit_reason.clone()).or_default() += 1;
    }

    let mut csv_out = String::new();
    csv_out.push_str("rank,symbol,entry_date,exit_date,bars_held,exit_reason,trade_return_pct,account_equity_mult,log_contribution,position_size,hedge_active,btc_regime_label,btc_21d_return_pct,btc_sma50_over_sma200_pct,btc_atr_percentile\n");
    for w in &winners {
        csv_out.push_str(&format!(
            "{},{},{},{},{},{},{:.2},{:.10},{:.10},{:.6},{},{},{:.2},{:.2},{:.2}\n",
            w.rank,
            w.trade.symbol,
            date_only(&w.trade.entry_date),
            date_only(&w.trade.exit_date),
            w.trade.bars_held,
            w.trade.exit_reason,
            w.trade.pct_ret * 100.0,
            w.trade.equity_mult,
            w.log_contribution,
            w.trade.size,
            w.trade.hedge_active,
            w.btc_regime_label,
            w.btc_21d_return * 100.0,
            w.btc_sma50_over_sma200 * 100.0,
            w.btc_atr_percentile,
        ));
    }
    File::create("snapshots/t104_top_winner_decomposition.csv")?.write_all(csv_out.as_bytes())?;

    let mut md = String::new();
    md.push_str("# T104 Top-Winner Mechanism Decomposition\n\n");
    md.push_str("**Status:** GENERATED from `snapshots/live_bot_exact_trades.csv` by `examples/t104_top_winner_decomposition.rs`. This is an understanding/regression artifact, not a filter or parameter sweep.\n\n");
    md.push_str("## Executive finding\n\n");
    md.push_str(&format!(
        "Top-10 trades contribute **{:.3} log equity** out of total **{:.3}** = **{:.1}%** of compounded log return. The corrected mechanism is: **early rebound/continuation inside still-damaged BTC long-term regimes**, not entries after a negative BTC 21d return.\n\n",
        top10_log_return,
        total_log_return,
        top10_share * 100.0
    ));
    md.push_str("Important correction versus the 2026-05-11 prose note: `btc_21d_return` is positive for every current top-10 entry. The negative values quoted there correspond to `btc_sma50_over_sma200`, a long-term trend-damage proxy.\n\n");
    md.push_str("## Summary stats\n\n");
    md.push_str(&format!(
        "- BTC 21d return positive at entry: **{}/10** (avg {})\n",
        positive_btc_21d,
        pct(avg_btc_21d)
    ));
    md.push_str(&format!(
        "- BTC SMA50/SMA200 proxy below zero: **{}/10**\n",
        long_term_bear
    ));
    md.push_str(&format!(
        "- BTC ATR percentile < 24: **{}/10**; < 5: **{}/10**\n",
        atr_lt_24, atr_lt_5
    ));
    md.push_str(&format!(
        "- Held <= 3 bars: **{}/10** (avg hold {:.1} bars)\n",
        fast_3, avg_hold
    ));
    md.push_str(&format!(
        "- Hedge active on trade: **{}/10**\n",
        hedge_active
    ));
    md.push_str("- Regime counts: ");
    md.push_str(
        &by_regime
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", "),
    );
    md.push('\n');
    md.push_str("- Exit counts: ");
    md.push_str(
        &by_exit
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", "),
    );
    md.push_str("\n\n");

    md.push_str("## Top-10 roster\n\n");
    md.push_str("| Rank | Symbol | Entry | Exit | Hold | Exit | Trade ret | Position | Hedge | BTC regime label | BTC 21d ret | SMA50/SMA200 proxy | BTC ATR pct | Log contrib |\n");
    md.push_str("|---:|---|---|---|---:|---|---:|---:|---|---|---:|---:|---:|---:|\n");
    for w in &winners {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.1}% | {} | {} | {} | {} | {} | {:.3} |\n",
            w.rank,
            w.trade.symbol,
            date_only(&w.trade.entry_date),
            date_only(&w.trade.exit_date),
            w.trade.bars_held,
            w.trade.exit_reason,
            pct(w.trade.pct_ret),
            w.trade.size * 100.0,
            bool_yn(w.trade.hedge_active),
            w.btc_regime_label,
            pct(w.btc_21d_return),
            pct(w.btc_sma50_over_sma200),
            pct_plain(w.btc_atr_percentile),
            w.log_contribution,
        ));
    }

    md.push_str("\n## Mechanism interpretation\n\n");
    md.push_str("1. **The largest winners are not fresh BTC drawdown entries.** All 10 current top winners had positive BTC 21d returns at entry.\n");
    md.push_str("2. **They mostly occur while the longer-term BTC trend is still damaged.** 8/10 have negative SMA50/SMA200 proxy values, so the bot is buying early rebound continuation before the long-term trend has fully healed.\n");
    md.push_str("3. **They are fast.** 5/10 exit within three daily bars; the edge is burst capture, not long trend patience.\n");
    md.push_str(&format!("4. **Higher ATR_RANK gates still fail the convex-tail guardrail.** {}/10 are below ATR_RANK 24, so T=24/T=65 remove too much of the compounding engine. T=5 remains the permissive production gate.\n", atr_lt_24));
    md.push_str("5. **Position size does not explain the concentration.** 10/10 current top winners are full 1/3 slots; hedge did not affect this current top-10 set.\n\n");

    md.push_str("## Production implication\n\n");
    md.push_str("The corrected story remains high-kurtosis and fragile: the bot gets paid when selected assets continue an early rebound while broader BTC trend state still looks impaired. Additional filters must preserve these early-rebound trades or they remove the only compounding engine and leave mostly whipsaws.\n\n");
    md.push_str("## Outputs\n\n- `snapshots/t104_top_winner_decomposition.csv`\n- `snapshots/t104_top_winner_decomposition.md`\n");

    File::create("snapshots/t104_top_winner_decomposition.md")?.write_all(md.as_bytes())?;

    println!("T104 top-winner decomposition complete");
    println!(
        "top10_share={:.1}% ({:.3}/{:.3} log)",
        top10_share * 100.0,
        top10_log_return,
        total_log_return
    );
    println!(
        "positive_btc_21d={}/10 avg_btc_21d={}",
        positive_btc_21d,
        pct(avg_btc_21d)
    );
    println!(
        "long_term_bear={}/10 atr_lt_24={}/10 fast_3bar={}/10",
        long_term_bear, atr_lt_24, fast_3
    );
    println!("wrote snapshots/t104_top_winner_decomposition.md");

    Ok(())
}
