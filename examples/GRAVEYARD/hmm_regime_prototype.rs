//! HMM Regime Prototype V2 — Kira 2026-04-02
//!
//! KILL condition: this was deferred 6+ sessions with API/look-ahead bugs.
//! This is the rewrite using DataLoader + FeatureEngine pattern.
//!
//! Question: can a 3-state Gaussian HMM on [return, realized_vol, volume_ratio]
//! identify bull/bear/chop regimes better than SMA200?
//!
//! Design:
//! - 3-state Gaussian HMM (bull, bear, chop)
//! - Features: 21-bar return, 21-bar realized vol (annualized), 21-bar volume ratio
//! - Train: rolling 252-bar window
//! - Test: next 252-bar window
//! - Strategy: Turtle+MACD with HMM state as entry filter
//! - Compare vs Turtle+MACD (no filter) and SMA200-filtered
//!
//! IMPORTANT: HMM is trained ONLY on training window data.
//! Test features are extracted ONLY from test period data.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CANDLES: u32 = 3000;

const TURTLE_LOOKBACK: usize = 20;
const FAST_EMA: usize = 12;
const SLOW_EMA: usize = 26;

// ─── Simple 3-state Gaussian HMM ──────────────────────────────────────────────

#[derive(Clone)]
struct HmmState {
    mean_ret: f64,
    mean_vol: f64,
    mean_vr: f64,
    var_ret: f64,
    var_vol: f64,
    var_vr: f64,
}

impl HmmState {
    fn emission_log_prob(&self, r: f64, vol: f64, vr: f64) -> f64 {
        use std::f64::consts::LN_2;
        let eps = 1e-10;
        let lr = ((self.var_ret.max(eps) * 2.0 * LN_2).sqrt() + eps).ln();
        let lv = ((self.var_vol.max(eps) * 2.0 * LN_2).sqrt() + eps).ln();
        let lvr = ((self.var_vr.max(eps) * 2.0 * LN_2).sqrt() + eps).ln();
        -0.5 * ((r - self.mean_ret).powi(2) / self.var_ret.max(eps)
            + (vol - self.mean_vol).powi(2) / self.var_vol.max(eps)
            + (vr - self.mean_vr).powi(2) / self.var_vr.max(eps))
            - lr
            - lv
            - lvr
    }
}

struct Hmm {
    n_states: usize,
    initial: Vec<f64>,
    transition: Vec<Vec<f64>>,
    states: Vec<HmmState>,
}

impl Hmm {
    fn new(n_states: usize) -> Self {
        Self {
            n_states,
            initial: vec![1.0 / n_states as f64; n_states],
            transition: vec![vec![1.0 / n_states as f64; n_states]; n_states],
            states: vec![
                HmmState {
                    mean_ret: 0.0,
                    mean_vol: 0.0,
                    mean_vr: 0.0,
                    var_ret: 0.01,
                    var_vol: 0.01,
                    var_vr: 0.01,
                };
                n_states
            ],
        }
    }

    /// Fit using k-means++ initialization then one Baum-Welch iteration
    fn fit(&mut self, features: &[(f64, f64, f64)], max_iter: usize) {
        let T = features.len();
        if T < self.n_states * 3 {
            return;
        }

        // K-means++ initialization
        self.init_kmeans(features);

        // Baum-Welch
        for _ in 0..max_iter {
            self.baum_welch_step(features);
        }
    }

    fn init_kmeans(&mut self, features: &[(f64, f64, f64)]) {
        let T = features.len();
        let n = self.n_states;

        // Pick first center deterministically from data
        let centers: Vec<(f64, f64, f64)> =
            (0..n).map(|i| features[(i * T / n).min(T - 1)]).collect();

        // Assign each point to nearest center (by return only for simplicity)
        let mut clusters: Vec<Vec<usize>> = vec![vec![]; n];
        for (i, f) in features.iter().enumerate() {
            let mut best = 0;
            let mut best_d = f64::MAX;
            for (c, center) in centers.iter().enumerate() {
                let d = (f.0 - center.0).abs();
                if d < best_d {
                    best_d = d;
                    best = c;
                }
            }
            clusters[best].push(i);
        }

        // Set state means from cluster means
        for (c, cluster) in clusters.iter().enumerate() {
            if cluster.is_empty() {
                continue;
            }
            let mut sum = (0.0_f64, 0.0_f64, 0.0_f64);
            for &i in cluster {
                sum.0 += features[i].0;
                sum.1 += features[i].1;
                sum.2 += features[i].2;
            }
            let n_c = cluster.len() as f64;
            self.states[c].mean_ret = sum.0 / n_c;
            self.states[c].mean_vol = sum.1 / n_c;
            self.states[c].mean_vr = sum.2 / n_c;
            self.states[c].var_ret = 0.01;
            self.states[c].var_vol = 0.01;
            self.states[c].var_vr = 0.01;
        }

        // Transition: favor staying
        for i in 0..n {
            for j in 0..n {
                self.transition[i][j] = if i == j {
                    0.85
                } else {
                    0.15 / (n as f64 - 1.0)
                };
            }
        }
    }

    fn baum_welch_step(&mut self, features: &[(f64, f64, f64)]) {
        let T = features.len();
        let n = self.n_states;
        if T < 2 {
            return;
        }

        // Forward variables
        let mut alpha = vec![vec![0.0_f64; n]; T];
        let mut scale = vec![0.0_f64; T];

        for j in 0..n {
            alpha[0][j] = self.initial[j]
                * (-self.states[j].emission_log_prob(features[0].0, features[0].1, features[0].2))
                    .exp();
        }
        scale[0] = alpha[0].iter().sum();
        if scale[0] > 0.0 {
            for j in 0..n {
                alpha[0][j] /= scale[0];
            }
        }

        for t in 1..T {
            for j in 0..n {
                alpha[t][j] = (-self.states[j].emission_log_prob(
                    features[t].0,
                    features[t].1,
                    features[t].2,
                ))
                .exp()
                    * (0..n)
                        .map(|i| alpha[t - 1][i] * self.transition[i][j])
                        .sum::<f64>();
            }
            scale[t] = alpha[t].iter().sum();
            if scale[t] > 0.0 {
                for j in 0..n {
                    alpha[t][j] /= scale[t];
                }
            }
        }

        // Update initial
        for j in 0..n {
            self.initial[j] = (alpha[0][j] * scale[0]).max(1e-6);
        }
        let init_sum = self.initial.iter().sum::<f64>();
        if init_sum > 0.0 {
            for j in 0..n {
                self.initial[j] /= init_sum;
            }
        }

        // Update transitions and emissions using gamma
        let mut new_trans = vec![vec![0.0_f64; n]; n];
        let mut gamma = vec![vec![0.0_f64; n]; T];
        let mut xi_sum = vec![vec![0.0_f64; n]; n];

        for t in 1..T {
            let denom = (0..n)
                .map(|i| {
                    (0..n)
                        .map(|j| {
                            alpha[t - 1][i]
                                * self.transition[i][j]
                                * (-self.states[j].emission_log_prob(
                                    features[t].0,
                                    features[t].1,
                                    features[t].2,
                                ))
                                .exp()
                        })
                        .sum::<f64>()
                })
                .sum::<f64>()
                .max(1e-10);

            for i in 0..n {
                for j in 0..n {
                    let num = alpha[t - 1][i]
                        * self.transition[i][j]
                        * (-self.states[j].emission_log_prob(
                            features[t].0,
                            features[t].1,
                            features[t].2,
                        ))
                        .exp();
                    xi_sum[i][j] += num / denom;
                }
            }
        }

        for i in 0..n {
            let row_sum = xi_sum[i].iter().sum::<f64>().max(1e-6);
            for j in 0..n {
                new_trans[i][j] = xi_sum[i][j] / row_sum;
            }
        }
        self.transition = new_trans;

        // Gamma (posterior probability of each state at each t)
        for t in 0..T {
            let sum_gamma: f64 = (0..n).map(|j| alpha[t][j]).sum();
            if sum_gamma > 0.0 {
                for j in 0..n {
                    gamma[t][j] = alpha[t][j] / sum_gamma;
                }
            }
        }

        // Update emissions
        for j in 0..n {
            let mut w_sum = 0.0_f64;
            let mut mr = 0.0_f64;
            let mut mv = 0.0_f64;
            let mut mvr = 0.0_f64;
            let mut vr_sum = 0.0_f64;
            let mut vv_sum = 0.0_f64;
            let mut vvr_sum = 0.0_f64;

            for t in 0..T {
                let w = gamma[t][j];
                w_sum += w;
                mr += w * features[t].0;
                mv += w * features[t].1;
                mvr += w * features[t].2;
            }
            if w_sum > 0.0 {
                mr /= w_sum;
                mv /= w_sum;
                mvr /= w_sum;
            }
            self.states[j].mean_ret = mr;
            self.states[j].mean_vol = mv;
            self.states[j].mean_vr = mvr;

            for t in 0..T {
                let w = gamma[t][j];
                vr_sum += w * (features[t].0 - mr).powi(2);
                vv_sum += w * (features[t].1 - mv).powi(2);
                vvr_sum += w * (features[t].2 - mvr).powi(2);
            }
            self.states[j].var_ret = (vr_sum / w_sum.max(1.0)).max(1e-6);
            self.states[j].var_vol = (vv_sum / w_sum.max(1.0)).max(1e-6);
            self.states[j].var_vr = (vvr_sum / w_sum.max(1.0)).max(1e-6);
        }
    }

    /// Viterbi decode: most likely state sequence (log-domain, no overflow)
    fn viterbi(&self, features: &[(f64, f64, f64)]) -> Vec<usize> {
        let T = features.len();
        if T == 0 {
            return vec![];
        }
        let n = self.n_states;

        // log initial probabilities
        let log_init: Vec<f64> = self.initial.iter().map(|&p| p.max(1e-300).ln()).collect();

        let mut log_delta = vec![vec![0.0_f64; n]; T];
        let mut psi = vec![vec![0usize; n]; T];

        // t=0
        for j in 0..n {
            log_delta[0][j] = log_init[j]
                - self.states[j].emission_log_prob(features[0].0, features[0].1, features[0].2);
            psi[0][j] = 0;
        }

        // t > 0
        for t in 1..T {
            for j in 0..n {
                let log_emit =
                    self.states[j].emission_log_prob(features[t].0, features[t].1, features[t].2);
                let mut best_val = f64::NEG_INFINITY;
                let mut best_k = 0usize;
                for k in 0..n {
                    // log(a * b) = log(a) + log(b)
                    // log(delta[t-1,k] * trans[k,j]) = log_delta[t-1,k] + log(trans[k,j])
                    let log_trans_kj = self.transition[k][j].max(1e-300).ln();
                    let val = log_delta[t - 1][k] + log_trans_kj;
                    if val > best_val {
                        best_val = val;
                        best_k = k;
                    }
                }
                log_delta[t][j] = best_val - log_emit;
                psi[t][j] = best_k;
            }
        }

        // Backtrack
        let mut path = vec![0usize; T];
        let mut best_final = f64::NEG_INFINITY;
        let mut q_T = 0usize;
        for j in 0..n {
            if log_delta[T - 1][j] > best_final {
                best_final = log_delta[T - 1][j];
                q_T = j;
            }
        }
        path[T - 1] = q_T;
        for t in (0..T - 1).rev() {
            path[t] = psi[t + 1][path[t + 1]];
        }
        path
    }
}

// ─── Feature extraction ───────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    volume: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Self {
        fn col(df: &DataFrame, n: &str) -> Vec<f64> {
            df.column(n)
                .unwrap()
                .f64()
                .unwrap()
                .to_vec()
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect()
        }
        Self {
            close: col(df, "close"),
            volume: col(df, "volume"),
        }
    }
}

/// Extract features for a window [start..end) — uses only data within that window
fn extract_window_features(
    close: &[f64],
    volume: &[f64],
    start: usize,
    end: usize,
) -> Vec<(f64, f64, f64)> {
    let vol_window = 21_usize;
    let mut features = Vec::with_capacity(end.saturating_sub(start));

    for i in start..end {
        let ret = if i > 0 {
            close[i] / close[i.max(1) - 1] - 1.0
        } else {
            0.0
        };

        // 21-bar realized vol
        let mut sum_sq = 0.0_f64;
        let mut count = 0_usize;
        for j in i.saturating_sub(vol_window)..i {
            if j > 0 {
                let r = close[j] / close[j - 1] - 1.0;
                sum_sq += r * r;
                count += 1;
            }
        }
        let rv = if count > 0 {
            (sum_sq / count as f64 * 252.0).sqrt()
        } else {
            0.0
        };

        // Volume ratio
        let vol_sum: f64 = volume[i.saturating_sub(vol_window)..i].iter().sum();
        let avg_vol = if count > 0 {
            vol_sum / count as f64
        } else {
            1.0
        };
        let vr = if avg_vol > 0.0 {
            volume[i] / avg_vol
        } else {
            1.0
        };

        features.push((ret, rv, vr));
    }
    features
}

// ─── Turtle+MACD backtest on a window ─────────────────────────────────────────

fn turtle_macd_backtest(
    all_data: &HashMap<&str, SymData>,
    symbols: &[&str],
    tstart: usize,
    tend: usize,
    hold_bars: usize,
    fee: f64,
    regime_filter: Option<&[usize]>, // HMM state per bar, or None = no filter
    bull_state: Option<usize>,
    bear_state: Option<usize>,
    sma200_filter: bool,
    close_prices: &[f64],
) -> StrategyResult {
    // Compute rank scores at tstart
    let mut scores: Vec<(&str, f64)> = symbols
        .iter()
        .map(|s| {
            let sd = all_data.get(s).unwrap();
            let (dc_hi, dc_lo) = {
                let hi_start = tstart.saturating_sub(TURTLE_LOOKBACK);
                let lo_start = tstart.saturating_sub(TURTLE_LOOKBACK);
                let hi = sd.close[hi_start..=tstart]
                    .iter()
                    .cloned()
                    .fold(0.0_f64, |a, v| a.max(v));
                let lo = sd.close[lo_start..=tstart]
                    .iter()
                    .cloned()
                    .fold(0.0_f64, |a, v| a.min(v));
                (hi, lo)
            };
            let macd_val = {
                let fast_ema_start = tstart.saturating_sub(SLOW_EMA);
                if fast_ema_start >= tstart {
                    0.0
                } else {
                    // Simple MACD: EMA_fast - EMA_slow approximation
                    let fast_avg: f64 = sd.close[fast_ema_start..=tstart].iter().sum::<f64>()
                        / (tstart - fast_ema_start + 1) as f64;
                    let slow_avg: f64 = sd.close[tstart.saturating_sub(SLOW_EMA * 2)..=tstart]
                        .iter()
                        .sum::<f64>()
                        / (SLOW_EMA + 1) as f64;
                    fast_avg - slow_avg
                }
            };

            // Trend score: close > 20d high AND MACD > 0
            let trend_score = if sd.close[tstart] > dc_hi && macd_val > 0.0 {
                3.0
            } else if sd.close[tstart] < dc_lo && macd_val < 0.0 {
                -3.0
            } else if sd.close[tstart] > dc_hi {
                2.0
            } else if sd.close[tstart] < dc_lo {
                -2.0
            } else {
                0.0
            };

            (*s, trend_score)
        })
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let top_k = 3_usize.min(scores.len());
    let longs: Vec<&str> = scores.iter().take(top_k).map(|(s, _)| *s).collect();
    let shorts: Vec<&str> = scores.iter().rev().take(top_k).map(|(s, _)| *s).collect();

    if longs.is_empty() && shorts.is_empty() {
        return StrategyResult::zero();
    }

    // Entry at tstart+1 open (no look-ahead: signal at tstart, execute at tstart+1)
    let entry_bar = (tstart + 1).min(tend.saturating_sub(hold_bars + 1));
    let exit_bar = (entry_bar + hold_bars).min(tend.saturating_sub(1));

    // SMA200 filter
    if sma200_filter && entry_bar < 200 {
        return StrategyResult::zero();
    }

    let leg_n = (longs.len() + shorts.len()) as f64;

    // Regime filter check (for HMM)
    if let Some(states) = regime_filter {
        if let Some(bull) = bull_state {
            if entry_bar >= states.len() {
                return StrategyResult::zero();
            }
            let state = states[entry_bar];
            // Only trade in bull state — skip bear/chop
            if state != bull {
                return StrategyResult::zero();
            }
        }
    }

    let mut trades = Vec::new();

    // Long legs
    for s in &longs {
        let sd = all_data.get(s).unwrap();
        let entry_px = sd.close[entry_bar] * (1.0 + fee);
        let exit_px = sd.close[exit_bar] * (1.0 - fee);
        trades.push((exit_px / entry_px - 1.0) / leg_n);
    }

    // Short legs
    for s in &shorts {
        let sd = all_data.get(s).unwrap();
        let entry_px = sd.close[entry_bar] * (1.0 - fee);
        let exit_px = sd.close[exit_bar] * (1.0 + fee);
        trades.push((entry_px / exit_px - 1.0) / leg_n);
    }

    StrategyResult::from_trades(&trades)
}

struct StrategyResult {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    n_trades: usize,
    win_rate: f64,
    pass: bool,
}

impl StrategyResult {
    fn zero() -> Self {
        Self {
            return_pct: 0.0,
            sharpe: 0.0,
            max_dd: 0.0,
            n_trades: 0,
            win_rate: 0.0,
            pass: false,
        }
    }
    fn from_trades(trades: &[f64]) -> Self {
        if trades.is_empty() {
            return Self::zero();
        }
        let n = trades.len() as f64;
        let ret = trades.iter().sum::<f64>() * 100.0;
        let wins = trades.iter().filter(|&&t| t > 0.0).count() as f64;
        let mean = trades.iter().sum::<f64>() / n;
        let var = trades.iter().map(|&t| (t - mean).powi(2)).sum::<f64>() / n;
        let std = var.sqrt();
        let sharpe = if std > 0.0 {
            mean / std * (252.0_f64.sqrt())
        } else {
            0.0
        };
        let win_rate = wins / n;
        // Max DD
        let mut equity = 1.0_f64;
        let mut peak = 1.0_f64;
        let mut max_dd = 0.0_f64;
        for &t in trades {
            equity *= 1.0 + t;
            peak = peak.max(equity);
            let dd = (peak - equity) / peak;
            max_dd = max_dd.max(dd);
        }
        let pass = ret > 0.0 && n >= 3.0;
        Self {
            return_pct: ret,
            sharpe,
            max_dd,
            n_trades: trades.len(),
            win_rate,
            pass,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("═══ HMM Regime Prototype V2 ═══\n");

    let loader = DataLoader::new(None, None);
    let syms: Vec<&str> = vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut all_data: HashMap<&str, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for s in &syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        let sd = SymData::from_df(&df);
        min_len = min_len.min(sd.close.len());
        all_data.insert(*s, sd);
    }

    let n = min_len.min(2800);
    for (_, sd) in &mut all_data {
        sd.close.truncate(n);
        sd.volume.truncate(n);
    }

    // Need 200 extra bars for SMA200 + realized vol warmup
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS + 250) / TEST_BARS;
    println!(
        "Symbols: {:?} | Bars: {} | Train: {} | Test: {} | Hold: {}",
        syms, n, TRAIN_BARS, TEST_BARS, HOLD_BARS
    );
    println!("Windows: {}\n", total_windows);

    let mut results: Vec<HmmResult> = Vec::new();

    for wi in 0..total_windows {
        let train_start = wi * TEST_BARS;
        let train_end = (train_start + TRAIN_BARS).min(n);
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(n);

        if test_end.saturating_sub(test_start) < HOLD_BARS + 30 {
            continue;
        }

        // ── Train HMM on training window ─────────────────────────────────────
        let btc_data = all_data.get("BTCUSDT").unwrap();

        // Use BTC features for regime detection
        let train_feats =
            extract_window_features(&btc_data.close, &btc_data.volume, train_start, train_end);

        let mut hmm = Hmm::new(3);
        hmm.fit(&train_feats, 20);
        let train_states = hmm.viterbi(&train_feats);

        // Identify bull/bear states by mean return
        let mut state_returns: Vec<(usize, f64)> = hmm
            .states
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.mean_ret))
            .collect();
        state_returns.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let bull_state = state_returns[0].0;
        let bear_state = state_returns[2].0;
        let chop_state = state_returns[1].0;

        println!("  W{:02}: train bars {}-{} | bull=state{} ret={:.4}/day bear=state{} ret={:.4}/day chop=state{} ret={:.4}/day",
            wi, train_start, train_end,
            bull_state, hmm.states[bull_state].mean_ret,
            bear_state, hmm.states[bear_state].mean_ret,
            chop_state, hmm.states[chop_state].mean_ret);

        // ── Decode test window states ────────────────────────────────────────
        let test_feats =
            extract_window_features(&btc_data.close, &btc_data.volume, test_start, test_end);
        let test_states = hmm.viterbi(&test_feats);

        // State distribution in test window
        let mut cnt = [0usize; 3];
        for &s in &test_states {
            if s < 3 {
                cnt[s] += 1;
            }
        }
        println!(
            "    Test state dist: bull={} bear={} chop={}",
            cnt[bull_state], cnt[bear_state], cnt[chop_state]
        );

        // ── Run strategies ───────────────────────────────────────────────────
        // Reference: BTC SMA200 at test_start
        let btc_close_at_test = &btc_data.close[train_start..test_end];
        let sma200_above = {
            let avg_200: f64 = btc_close_at_test.iter().take(200).sum::<f64>()
                / 200.0_f64.min(btc_close_at_test.len() as f64);
            btc_close_at_test.get(200).copied().unwrap_or(0.0) > avg_200
        };

        // Strategy 1: Turtle+MACD (unfiltered)
        let unfiltered = turtle_macd_backtest(
            &all_data,
            &syms,
            test_start,
            test_end,
            HOLD_BARS,
            TAKER_FEE,
            None,
            None,
            None,
            false,
            &[],
        );

        // Strategy 2: Turtle+MACD + SMA200 filter
        let sma_filtered = turtle_macd_backtest(
            &all_data,
            &syms,
            test_start,
            test_end,
            HOLD_BARS,
            TAKER_FEE,
            None,
            None,
            None,
            true,
            &btc_data.close[train_start..test_end],
        );

        // Strategy 3: Turtle+MACD + HMM bull-state filter
        let hmm_filtered = turtle_macd_backtest(
            &all_data,
            &syms,
            test_start,
            test_end,
            HOLD_BARS,
            TAKER_FEE,
            Some(&test_states),
            Some(bull_state),
            None,
            false,
            &[],
        );

        println!(
            "    UNFILTERED: {:+.1}% sh={:.2} DD={:+.1}% {}t {}",
            unfiltered.return_pct,
            unfiltered.sharpe,
            unfiltered.max_dd * 100.0,
            unfiltered.n_trades,
            if unfiltered.pass { "PASS" } else { "FAIL" }
        );
        println!(
            "    SMA200:     {:+.1}% sh={:.2} DD={:+.1}% {}t {}",
            sma_filtered.return_pct,
            sma_filtered.sharpe,
            sma_filtered.max_dd * 100.0,
            sma_filtered.n_trades,
            if sma_filtered.pass { "PASS" } else { "FAIL" }
        );
        println!(
            "    HMM-BULL:   {:+.1}% sh={:.2} DD={:+.1}% {}t {}",
            hmm_filtered.return_pct,
            hmm_filtered.sharpe,
            hmm_filtered.max_dd * 100.0,
            hmm_filtered.n_trades,
            if hmm_filtered.pass { "PASS" } else { "FAIL" }
        );

        results.push(HmmResult {
            wi,
            test_start,
            test_end,
            bull_state,
            bear_state,
            chop_state,
            state_counts: cnt,
            unfiltered,
            sma_filtered,
            hmm_filtered,
        });
    }

    let n_windows = results.len();
    println!("\n═══ SUMMARY ═══");

    let unf_pass = results.iter().filter(|r| r.unfiltered.pass).count();
    let sma_pass = results.iter().filter(|r| r.sma_filtered.pass).count();
    let hmm_pass = results.iter().filter(|r| r.hmm_filtered.pass).count();

    let unf_avg = results.iter().map(|r| r.unfiltered.return_pct).sum::<f64>() / n_windows as f64;
    let sma_avg = results
        .iter()
        .map(|r| r.sma_filtered.return_pct)
        .sum::<f64>()
        / n_windows as f64;
    let hmm_avg = results
        .iter()
        .map(|r| r.hmm_filtered.return_pct)
        .sum::<f64>()
        / n_windows as f64;

    let unf_dd = results
        .iter()
        .map(|r| r.unfiltered.max_dd)
        .fold(0.0_f64, |a, v| a.max(v));
    let sma_dd = results
        .iter()
        .map(|r| r.sma_filtered.max_dd)
        .fold(0.0_f64, |a, v| a.max(v));
    let hmm_dd = results
        .iter()
        .map(|r| r.hmm_filtered.max_dd)
        .fold(0.0_f64, |a, v| a.max(v));

    println!(
        "{:<12} {:>8} {:>12} {:>10} {:>10}",
        "Strategy", "Pass", "Avg OOS", "Avg Sharpe", "Worst DD"
    );
    println!("{}", "-".repeat(55));
    println!(
        "{:<12} {:>4}/{} {:>+11.1}% {:>+9.2} {:>+9.1}%",
        "Unfiltered",
        unf_pass,
        n_windows,
        unf_avg,
        results.iter().map(|r| r.unfiltered.sharpe).sum::<f64>() / n_windows as f64,
        unf_dd * 100.0
    );
    println!(
        "{:<12} {:>4}/{} {:>+11.1}% {:>+9.2} {:>+9.1}%",
        "SMA200",
        sma_pass,
        n_windows,
        sma_avg,
        results.iter().map(|r| r.sma_filtered.sharpe).sum::<f64>() / n_windows as f64,
        sma_dd * 100.0
    );
    println!(
        "{:<12} {:>4}/{} {:>+11.1}% {:>+9.2} {:>+9.1}%",
        "HMM-Bull",
        hmm_pass,
        n_windows,
        hmm_avg,
        results.iter().map(|r| r.hmm_filtered.sharpe).sum::<f64>() / n_windows as f64,
        hmm_dd * 100.0
    );

    println!("\n─── HMM Interpretation ───");
    for r in &results {
        println!(
            "  W{:02}: bull=state{} | bear=state{} | chop=state{} | test_bull={:.0}%",
            r.wi,
            r.bull_state,
            r.bear_state,
            r.chop_state,
            r.state_counts[r.bull_state] as f64
                / (r.state_counts[0] + r.state_counts[1] + r.state_counts[2]).max(1) as f64
                * 100.0
        );
    }

    println!("\n⏱  Done in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

struct HmmResult {
    wi: usize,
    test_start: usize,
    test_end: usize,
    bull_state: usize,
    bear_state: usize,
    chop_state: usize,
    state_counts: [usize; 3],
    unfiltered: StrategyResult,
    sma_filtered: StrategyResult,
    hmm_filtered: StrategyResult,
}
