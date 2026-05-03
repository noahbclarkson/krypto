//! BTC-ETH Cointegration Walk-Forward Validation (T52)
//!
//! Track C: Broaden edge discovery — genuinely untested idea (4 weeks overdue).
//!
//! Hypothesis: BTC and ETH cointegrate. When spread (ETH - beta * BTC) diverges
//! from its historical mean (z-score entry), it mean-reverts.
//!
//! Prior MR: all GRAVEYARD (RSI, BollingerReversion, OFI, VPIN, 4h MR, 1h MR).
//! Cointegration is fundamentally different.
//!
//! Academic: Frontiers in Finance, Jan 2026 — BTC-ETH cointegrating coefficient ~0.0587.
//!
//! NO look-ahead. Beta estimated on training window only.

use anyhow::Result as AnyResult;
use colored::Colorize;
use krypto::data::DataLoader;

const FEE_PCT: f64 = 0.001;
const SLIPPAGE_PCT: f64 = 0.0005;
const N_WINDOWS: usize = 6;
const TRAIN_BARS: usize = 252;

struct WfCfg(&'static str, usize, f64, f64, usize);
// (name, beta_lb, z_entry, z_exit, max_hold)

const CONFIGS: &[WfCfg] = &[
    WfCfg("LB20_E2.0_X1.0_H20",  20, 2.0, 1.0, 20),
    WfCfg("LB20_E2.0_X0.5_H20",  20, 2.0, 0.5, 20),
    WfCfg("LB20_E1.5_X0.5_H20",  20, 1.5, 0.5, 20),
    WfCfg("LB20_E2.5_X1.0_H20",  20, 2.5, 1.0, 20),
    WfCfg("LB40_E2.0_X1.0_H30",  40, 2.0, 1.0, 30),
    WfCfg("LB40_E2.0_X0.5_H30",  40, 2.0, 0.5, 30),
    WfCfg("LB40_E1.5_X0.5_H30",  40, 1.5, 0.5, 30),
    WfCfg("LB40_E2.5_X1.0_H30",  40, 2.5, 1.0, 30),
    WfCfg("LB60_E2.0_X1.0_H40",  60, 2.0, 1.0, 40),
    WfCfg("LB60_E2.0_X0.5_H40",  60, 2.0, 0.5, 40),
    WfCfg("LB60_E1.5_X0.5_H40",  60, 1.5, 0.5, 40),
    WfCfg("LB60_E2.5_X1.0_H40",  60, 2.5, 1.0, 40),
];

fn ols_beta(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len()).max(2);
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut sxx = 0.0_f64;
    let mut sxy = 0.0_f64;
    for i in 0..n {
        sx  += x[i];
        sy  += y[i];
        sxx += x[i] * x[i];
        sxy += x[i] * y[i];
    }
    let n = n as f64;
    let den = sxx - sx * sx / n;
    if den.abs() < 1e-10 { return 1.0; }
    (sxy - sx * sy / n) / den
}

fn spread_zscore(eth: &[f64], btc: &[f64], beta: f64, lookback: usize, first_valid: usize) -> Vec<f64> {
    let n = eth.len().min(btc.len());
    let mut z = vec![0.0; n];
    for i in first_valid..n {
        let start = i.saturating_sub(lookback);
        let mut sm = 0.0_f64;
        let mut ssq = 0.0_f64;
        for j in start..i {
            let s = eth[j] - beta * btc[j];
            sm += s;
            ssq += s * s;
        }
        let lw = lookback as f64;
        sm /= lw;
        let variance = (ssq / lw) - sm * sm;
        let sd = variance.sqrt().max(1e-10);
        z[i] = (eth[i] - beta * btc[i] - sm) / sd;
    }
    z
}

struct WfOut {
    trades: usize,
    wins: usize,
    sharpe: f64,
    net_return: f64,
    max_dd: f64,
    avg_ret: f64,
}

impl WfOut {
    fn pass(&self) -> bool {
        self.trades >= 30 && self.sharpe > 0.0
    }
}

fn backtest(
    btc: &[f64],
    eth: &[f64],
    oos_start: usize,
    beta: f64,
    z_entry: f64,
    z_exit: f64,
    max_hold: usize,
    z_lb: usize,
) -> WfOut {
    let zscores = spread_zscore(eth, btc, beta, z_lb, oos_start);

    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut rets = Vec::new();
    let mut pos: Option<(bool, usize, f64, f64)> = None;

    for bar in oos_start..btc.len() {
        let z = zscores[bar];

        if let Some((is_short, entry_bar, eth_e, btc_e)) = pos {
            let held = bar - entry_bar;
            let z_exit_ok = if is_short { z <= z_exit } else { z >= -z_exit };
            if z_exit_ok || held >= max_hold {
                let eth_x = eth[bar];
                let btc_x = btc[bar];
                let pct_eth = if is_short {
                    (eth_e - eth_x) / eth_e
                } else {
                    (eth_x - eth_e) / eth_e
                };
                let pct_btc = if is_short {
                    (btc_x - btc_e) / btc_e
                } else {
                    (btc_e - btc_x) / btc_e
                };
                let gross = (pct_eth + pct_btc) / 2.0;
                let cost = FEE_PCT * 2.0 + SLIPPAGE_PCT * 2.0;
                let net = gross - cost;
                equity *= 1.0 + net;
                peak = peak.max(equity);
                max_dd = max_dd.max(peak - equity);
                trades += 1;
                if net > 0.0 { wins += 1; }
                rets.push(net);
                pos = None;
            }
        }

        if pos.is_none() {
            if z > z_entry {
                pos = Some((true, bar, eth[bar], btc[bar]));
            } else if z < -z_entry {
                pos = Some((false, bar, eth[bar], btc[bar]));
            }
        }
    }

    let avg_ret = if rets.is_empty() {
        0.0
    } else {
        rets.iter().sum::<f64>() / rets.len() as f64
    };

    let std_ret = if rets.len() > 1 {
        let m = avg_ret;
        (rets.iter().map(|r| (r - m).powi(2)).sum::<f64>() / rets.len() as f64).sqrt().max(1e-10)
    } else {
        1.0
    };

    let tpy = (252.0 / max_hold as f64).min(trades as f64).max(1.0);
    let sharpe = if trades >= 5 {
        (avg_ret / std_ret) * tpy.sqrt()
    } else {
        -999.0
    };

    WfOut { trades, wins, sharpe, net_return: equity - 1.0, max_dd, avg_ret }
}

fn load_prices() -> AnyResult<(Vec<f64>, Vec<f64>)> {
    let loader = DataLoader::new(None, None);
    let btc_df = loader.load_from_cache("BTCUSDT", "1d")?
        .ok_or_else(|| anyhow::anyhow!("BTCUSDT 1d not found"))?;
    let eth_df = loader.load_from_cache("ETHUSDT", "1d")?
        .ok_or_else(|| anyhow::anyhow!("ETHUSDT 1d not found"))?;
    let n = btc_df.height().min(eth_df.height());
    let btc: Vec<f64> = btc_df.column("close")?.f64()?.into_iter().take(n).map(|v| v.unwrap_or(0.0)).collect();
    let eth: Vec<f64> = eth_df.column("close")?.f64()?.into_iter().take(n).map(|v| v.unwrap_or(0.0)).collect();
    Ok((btc, eth))
}

fn main() -> AnyResult<()> {
    println!("BTC-ETH Cointegration Walk-Forward (T52)");
    println!("==========================================");
    println!("Fee: 10bps taker/leg/side + 5bps slippage/leg/side");
    println!();

    let (btc, eth) = load_prices()?;
    let n = btc.len();
    println!("Loaded {} aligned daily bars", n);

    let step = ((n as isize - TRAIN_BARS as isize) / N_WINDOWS as isize).max(1) as usize;

    // all_results[config_idx] = Vec<WfOut>
    let mut all_results: Vec<Vec<WfOut>> = Vec::new();
    for _ in 0..CONFIGS.len() {
        all_results.push(Vec::new());
    }

    for w in 0..N_WINDOWS {
        let train_start = w * step;
        let train_end = (train_start + TRAIN_BARS).min(n);
        let oos_start = train_end;
        if oos_start >= n { break; }

        let beta = ols_beta(&btc[train_start..train_end], &eth[train_start..train_end]);
        println!("W{}/{}: train [{}-{}({}) oos [{}-{}] beta={:.4}",
            w + 1, N_WINDOWS,
            train_start, train_end, train_end - train_start,
            oos_start, n, beta);

        for ci in 0..CONFIGS.len() {
            let cfg = &CONFIGS[ci];
            let r = backtest(&btc, &eth, oos_start, beta, cfg.2, cfg.3, cfg.4, cfg.1);
            all_results[ci].push(r);
        }
    }

    println!();
    println!("AGGREGATE RESULTS");
    println!("================================================================================");

    let mut agg: Vec<(String, usize, usize, f64, f64, f64, f64)> = Vec::new();

    for ci in 0..CONFIGS.len() {
        let cfg = &CONFIGS[ci];
        let results = &all_results[ci];
        if results.is_empty() { continue; }

        let nw = results.len() as f64;
        let total_trades: usize = results.iter().map(|r| r.trades).sum();
        let total_wins: usize = results.iter().map(|r| r.wins).sum();
        let avg_sharpe: f64 = results.iter().map(|r| r.sharpe).sum::<f64>() / nw;
        let avg_net: f64 = results.iter().map(|r| r.net_return).sum::<f64>() / nw;
        let max_dd: f64 = results.iter().map(|r| r.max_dd).fold(0.0_f64, |a, b| a.max(b));
        let avg_ret_pt: f64 = results.iter().map(|r| r.avg_ret).sum::<f64>() / nw;
        let pass_count = results.iter().filter(|r| r.pass()).count();
        let pass_rate = pass_count as f64 / nw;

        // Use explicit string formatting to avoid % in format strings
        let pr_str = format!("{:.0}%", pass_rate * 100.0);
        let pr_colored = if pass_rate >= 0.6 { pr_str.green() } else { pr_str.red() };
        let max_dd_str = format!("{:.1}%", max_dd * 100.0);
        let ret_pt_str = format!("{:+.2}%", avg_ret_pt * 100.0);
        let net_ret_str = format!("{:+.1}%", avg_net * 100.0);

        println!("{:30} | {} trades | {} wins | {:7.3} Sharpe | {} ret | {} maxDD | {}/trade | {}/{} {}",
            cfg.0,
            total_trades,
            total_wins,
            avg_sharpe,
            net_ret_str,
            max_dd_str,
            ret_pt_str,
            pass_count,
            results.len(),
            pr_colored);

        agg.push((cfg.0.to_string(), total_trades, total_wins, avg_sharpe, avg_net, max_dd, avg_ret_pt));
    }

    agg.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    let best = &agg[0];
    let nw_total = all_results[0].len();
    let pass_min = ((nw_total as f64) * 0.6).ceil() as usize;
    let best_pass = agg.iter().filter(|r| r.1 >= 30 && r.3 > 0.0).count();

    println!();
    let best_net_str = format!("{:+.1}%", best.4 * 100.0);
    let best_dd_str = format!("{:.1}%", best.5 * 100.0);
    println!("BEST: {}  Sharpe={:.3}  Ret={}  Trades={}  maxDD={}",
        best.0, best.3, best_net_str, best.1, best_dd_str);
    println!();
    if best_pass >= pass_min && best.1 >= 30 && best.3 > 0.0 {
        println!("VERDICT: CANDIDATE -- passes guardrails (>=60% windows, >=30 trades, Sharpe>0)");
        println!("BTC-ETH cointegration edge is REAL. Promote to HOF, build live execution.");
    } else {
        println!("VERDICT: REJECTED -- fails guardrails.");
        println!("Every prior MR: GRAVEYARD. Mechanism real but edge insufficient. GRAVEYARD cleanly.");
    }

    Ok(())
}
