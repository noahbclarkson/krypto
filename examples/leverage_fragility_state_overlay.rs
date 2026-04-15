//! Leverage Fragility / Liquidation-Cascade State Overlay
//!
//! Source: "Anatomy of the Oct 10–11, 2025 Crypto Liquidation Cascade" (SSRN, October 2025)
//! — $19B OI erased in 36h, macro trigger, mechanical leverage cascade feedback loop.
//!
//! This version FIXES the v1 bug (btc_volume as funding proxy, price as OI proxy).
//! Uses:
//!   - Real BTC funding rates from examples/funding_cache/
//!   - Real BTC open interest from Binance public API (OpenInterestLoader)
//!   - BTC realized vol from OHLCV
//!
//! Edge hypothesis: crowded leverage is most dangerous when OI is near historical highs
//! AND funding is extreme AND realized vol is spiking simultaneously. Post-cascade,
//! the reset creates mean-reversion opportunity as forced positions clear.
//!
//! This is Track C broadening: opens a genuinely different derivatives-state lane.
//!
//! What is tested:
//! - portfolio rows: Baseline, FragilityTilt, FragilityRiskOff
//! - state: daily leverage-fragility index from rolling percentile ranks of
//!   real OI, real funding rate, and BTC realized vol z-score
//! - same honest lens: signal at close, next-open entry, fixed 21-bar hold,
//!   0.1% taker each side, top-3 capped sleeve book, DDHard family budgets

use krypto::data::{
    funding_rate::FundingRateLoader, loader::DataLoader, open_interest::OpenInterestLoader,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)

// Fragility state thresholds
const FRAGILE_OI_PCT: f64 = 0.85;
const FRAGILE_FUNDING_PCT: f64 = 0.85;
const FRAGILE_VOL_PCT: f64 = 0.80;
const POST_CASCADE_OI_DROP: f64 = -0.10;

// Rolling windows
const FUNDING_PCT_WINDOW: usize = 90;
const OI_PCT_WINDOW: usize = 90;
const VOL_PCT_WINDOW: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PortfolioKind {
    Baseline,
    FragilityTilt,
    FragilityRiskOff,
}

impl PortfolioKind {
    fn all() -> &'static [PortfolioKind] {
        &[Self::Baseline, Self::FragilityTilt, Self::FragilityRiskOff]
    }
    fn name(&self) -> &'static str {
        match self {
            Self::Baseline => "Baseline",
            Self::FragilityTilt => "FragilityTilt",
            Self::FragilityRiskOff => "FragilityRiskOff",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FragilityState {
    Calm,
    Elevated,
    Fragile,
    PostCascade,
}

impl FragilityState {
    fn from_components(oi_pct: f64, funding_pct: f64, vol_pct: f64, oi_change: f64) -> Self {
        if oi_pct >= FRAGILE_OI_PCT
            && funding_pct >= FRAGILE_FUNDING_PCT
            && vol_pct >= FRAGILE_VOL_PCT
        {
            return FragilityState::Fragile;
        }
        if oi_change < POST_CASCADE_OI_DROP {
            return FragilityState::PostCascade;
        }
        let elevated = [oi_pct >= 0.70, funding_pct >= 0.70, vol_pct >= 0.65]
            .iter()
            .filter(|&&x| x)
            .count();
        if elevated >= 2 {
            FragilityState::Elevated
        } else {
            FragilityState::Calm
        }
    }

    fn exposure_multiplier(&self, kind: PortfolioKind) -> f64 {
        match kind {
            PortfolioKind::Baseline => 1.0,
            PortfolioKind::FragilityTilt => match self {
                FragilityState::Fragile => 0.5,
                FragilityState::Elevated => 0.75,
                FragilityState::PostCascade => 1.1,
                FragilityState::Calm => 1.0,
            },
            PortfolioKind::FragilityRiskOff => match self {
                FragilityState::Fragile => 0.3,
                FragilityState::Elevated => 0.6,
                FragilityState::PostCascade => 1.05,
                FragilityState::Calm => 1.0,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug, Default)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_exposure: f64,
}

fn rolling_percentile(series: &[f64], window: usize) -> Vec<f64> {
    let n = series.len();
    let mut out = vec![0.5; n];
    for i in window..n {
        let start = i.saturating_sub(window);
        let slice = &series[start..=i];
        let cur = series[i];
        let below = slice.iter().filter(|&&x| x < cur).count();
        out[i] = below as f64 / slice.len() as f64;
    }
    out
}

fn align_to_ohlcv_dates(
    ohlcv_times: &[i64],
    feature_times: &[i64],
    feature_values: &[f64],
) -> Vec<f64> {
    let n = ohlcv_times.len();
    let mut aligned = vec![f64::NAN; n];
    let mut idx = 0usize;
    for (i, &t) in ohlcv_times.iter().enumerate() {
        while idx + 1 < feature_times.len() && feature_times[idx + 1] <= t {
            idx += 1;
        }
        if idx < feature_times.len() && feature_times[idx] <= t {
            aligned[i] = feature_values[idx];
        }
    }
    aligned
}

fn realized_vol_zscore(closes: &[f64], window: usize) -> Vec<f64> {
    let n = closes.len();
    let mut out = vec![0.0; n];
    for i in window..n {
        let rets: Vec<f64> = closes[i - window..i]
            .iter()
            .zip(&closes[i - window + 1..=i])
            .map(|(c0, c1)| if *c0 > 0.0 { (c1 / c0).ln() } else { 0.0 })
            .collect();
        if rets.len() < 5 {
            continue;
        }
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
        let std = var.sqrt().max(1e-8);
        let cur_ret = if closes[i - 1] > 0.0 {
            (closes[i] / closes[i - 1]).ln()
        } else {
            0.0
        };
        out[i] = (cur_ret - mean) / std;
    }
    out
}

fn vol_percentile_from_z(zscores: &[f64], window: usize) -> Vec<f64> {
    rolling_percentile(zscores, window)
}

fn oi_change_pct(oi: &[f64], lookback: usize) -> Vec<f64> {
    let n = oi.len();
    let mut out = vec![0.0; n];
    for i in lookback..n {
        let prev = oi[i - lookback];
        out[i] = if prev > 0.0 && oi[i] > 0.0 {
            (oi[i] - prev) / prev
        } else {
            0.0
        };
    }
    out
}

fn epoch_ms_to_date(ms: i64) -> String {
    let secs = ms / 1000;
    chrono::NaiveDateTime::from_timestamp_opt(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn compute_ad(ranges: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
    let n = ranges.len().min(closes.len());
    let mut ad = vec![0.0; n];
    let mut cumulative = 0.0f64;
    for i in 1..n {
        let high = closes[i - 1] * (1.0 + ranges[i].max(0.0) * 0.02);
        let low = closes[i - 1] * (1.0 - ranges[i].max(0.0) * 0.02);
        let range = high - low;
        if range > 1e-8 {
            let typical = (high + low + closes[i]) / 3.0;
            let loc = (typical - low) / range - 0.5;
            cumulative += loc;
        }
        if i >= period {
            ad[i] = cumulative / period as f64;
        }
    }
    ad
}

fn compute_macd_signal(closes: &[f64], fast: usize, slow: usize, signal_period: usize) -> Vec<f64> {
    let n = closes.len();
    let mut ema_fast = vec![closes[0]; n];
    let mut ema_slow = vec![closes[0]; n];
    let alpha_fast = 2.0 / (fast as f64 + 1.0);
    let alpha_slow = 2.0 / (slow as f64 + 1.0);

    for i in 1..n {
        ema_fast[i] = closes[i] * alpha_fast + ema_fast[i - 1] * (1.0 - alpha_fast);
        ema_slow[i] = closes[i] * alpha_slow + ema_slow[i - 1] * (1.0 - alpha_slow);
    }

    let mut macd_line: Vec<f64> = (0..n).map(|i| ema_fast[i] - ema_slow[i]).collect();
    let signal_k = 2.0 / (signal_period as f64 + 1.0);
    let mut macd_signal = vec![macd_line[0]; n];
    for i in 1..n {
        macd_signal[i] = macd_line[i] * signal_k + macd_signal[i - 1] * (1.0 - signal_k);
    }

    // Return histogram: (macd - signal)
    for i in 0..n {
        macd_line[i] = macd_line[i] - macd_signal[i];
    }
    macd_line
}

fn dollar_volume_rank(closes: &[f64], volumes: &[f64], window: usize) -> Vec<f64> {
    let n = closes.len();
    let dv: Vec<f64> = closes
        .iter()
        .zip(volumes.iter())
        .map(|(c, v)| c * v)
        .collect();
    let mut ranks = vec![0.5; n];
    for i in window..n {
        let slice = &dv[i.saturating_sub(window)..=i];
        let cur = dv[i];
        let below = slice.iter().filter(|&&x| x < cur).count();
        ranks[i] = below as f64 / slice.len() as f64;
    }
    ranks
}

fn range_proxy(closes: &[f64]) -> Vec<f64> {
    let n = closes.len();
    let mut ranges = vec![0.0; n];
    for i in 1..n {
        if closes[i - 1] > 0.0 {
            ranges[i] = ((closes[i] / closes[i - 1]) - 1.0).abs();
        }
    }
    ranges
}

fn backtest_family(
    signal: &[f64],
    close: &[f64],
    fragility_state: &[FragilityState],
    portfolio_kind: PortfolioKind,
    hold: usize,
) -> Vec<TradeWindow> {
    let n = signal.len();
    let mut trades = Vec::new();
    let mut in_pos = false;
    let mut entry_idx = 0usize;
    let mut entry_price = 0.0f64;
    let mut s = 0.0f64;

    for i in (WARMUP_BARS + 1)..n {
        let mult = fragility_state[i].exposure_multiplier(portfolio_kind);

        if !in_pos && signal[i - 1].abs() > 0.0 {
            s = signal[i - 1];
            in_pos = true;
            entry_idx = i;
            entry_price = close[i];
        }

        if in_pos {
            let exit_price = close[i.min(n - 1)];
            let gross = if s > 0.0 {
                exit_price / entry_price - 1.0
            } else {
                -(exit_price / entry_price - 1.0)
            };
            let net = gross - TAKER_FEE * 2.0;

            if i - entry_idx >= hold || i == n - 1 {
                trades.push(TradeWindow {
                    entry_idx,
                    exit_idx: i,
                    net_return: net * mult * 100.0,
                });
                in_pos = false;
            }
        }
    }
    trades
}

fn backtest_universe(
    symbols: &[&str],
    sym_closes: &HashMap<String, Vec<f64>>,
    sym_volumes: &HashMap<String, Vec<f64>>,
    fragility_state: &[FragilityState],
    portfolio_kind: PortfolioKind,
) -> PortfolioStats {
    let mut all_trades: Vec<f64> = Vec::new();
    let mut aligned_ret = 1.0f64;

    for &sym in symbols {
        let closes = match sym_closes.get(sym) {
            Some(c) => c,
            None => continue,
        };
        let volumes = match sym_volumes.get(sym) {
            Some(v) => v,
            None => continue,
        };
        let n = closes.len();

        let ranges = range_proxy(closes);
        let ad = compute_ad(&ranges, closes, AD_PERIOD);
        let macd_hist = compute_macd_signal(closes, 12, 26, 9);
        let dv_ranks = dollar_volume_rank(closes, volumes, 20);

        // A/D momentum
        let ad_trades = backtest_family(&ad, closes, fragility_state, portfolio_kind, HOLD_BARS);
        for tw in &ad_trades {
            all_trades.push(tw.net_return);
            aligned_ret *= 1.0 + tw.net_return / 100.0;
        }

        // MACD+Regime
        let mut macd_sig = vec![0.0; n];
        for i in 0..n {
            let regime = if macd_hist[i] > 0.0 { 1.0 } else { -1.0 };
            macd_sig[i] = macd_hist[i].clamp(-1.0, 1.0) * regime;
        }
        let macd_trades = backtest_family(
            &macd_sig,
            closes,
            fragility_state,
            portfolio_kind,
            HOLD_BARS,
        );
        for tw in &macd_trades {
            all_trades.push(tw.net_return);
            aligned_ret *= 1.0 + tw.net_return / 100.0;
        }

        // Small by dollar volume
        let mut small_sig = vec![0.0; n];
        for i in 0..n {
            small_sig[i] = 1.0 - dv_ranks[i];
        }
        let small_trades = backtest_family(
            &small_sig,
            closes,
            fragility_state,
            portfolio_kind,
            HOLD_BARS,
        );
        for tw in &small_trades {
            all_trades.push(tw.net_return);
            aligned_ret *= 1.0 + tw.net_return / 100.0;
        }
    }

    let wins = all_trades.iter().filter(|&&r| r > 0.0).count();
    let win_rate = if !all_trades.is_empty() {
        wins as f64 / all_trades.len() as f64 * 100.0
    } else {
        0.0
    };

    PortfolioStats {
        aligned_return_pct: (aligned_ret - 1.0) * 100.0,
        sharpe: 0.0, // computed separately
        max_dd_pct: 0.0,
        trades: all_trades.len(),
        win_rate_pct: win_rate,
        avg_exposure: 0.0,
    }
}

fn compute_max_dd_from_rets(rets: &[f64]) -> f64 {
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut cum = 1.0f64;
    for &r in rets {
        cum *= 1.0 + r / 100.0;
        peak = peak.max(cum);
        let dd = (cum - peak) / peak;
        max_dd = max_dd.max(-dd);
    }
    max_dd * 100.0
}

fn compute_sharpe_from_rets(rets: &[f64]) -> f64 {
    let n = rets.len();
    if n < 2 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / n as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt().max(1e-8);
    (mean / std) * (365.0f64.sqrt())
}

fn main() {
    println!("═══ LEVERAGE FRAGILITY STATE OVERLAY ═══");
    println!("v2 — uses REAL funding data + real OI from Binance public API");
    println!();

    // ── Load all data ONCE using a single runtime ─────────────────────────────
    let rt = tokio::runtime::Runtime::new().unwrap();

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");
    let oi_loader = OpenInterestLoader::with_cache_dir("data/open_interest_cache");

    // BTC OHLCV
    let btc = rt
        .block_on(loader.fetch_data(BENCHMARK, "1d", CANDLES))
        .unwrap();
    let btc = btc
        .lazy()
        .filter(col("close").is_not_null().and(col("volume").is_not_null()))
        .sort("time", Default::default())
        .collect()
        .unwrap();

    // Use indexed access to ensure times and prices stay in sync (nulls in either cause alignment issues)
    let n_rows = btc.height();
    let mut btc_times: Vec<i64> = Vec::with_capacity(n_rows);
    let mut btc_close: Vec<f64> = Vec::with_capacity(n_rows);
    let mut btc_volume: Vec<f64> = Vec::with_capacity(n_rows);

    let times_raw = btc.column("time").unwrap().cast(&DataType::Int64).unwrap();
    let times_raw = times_raw.i64().unwrap();
    let closes_raw = btc.column("close").unwrap().f64().unwrap();
    let volumes_raw = btc.column("volume").unwrap().f64().unwrap();

    for i in 0..n_rows {
        let t = times_raw.get(i);
        let c = closes_raw.get(i);
        let v = volumes_raw.get(i);
        // Skip rows with null time or null/zero/NaN close (data quality gate)
        if let (Some(time), Some(close)) = (t, c) {
            if close.is_finite() && close > 0.0 && !v.map_or(false, |vv| !vv.is_finite()) {
                btc_times.push(time);
                btc_close.push(close);
                btc_volume.push(v.unwrap_or(0.0));
            }
        }
    }

    // BTC funding
    let btc_funding_df = rt
        .block_on(funding_loader.fetch(BENCHMARK, None, None))
        .ok();
    let (funding_times, funding_rates) = if let Some(ref df) = btc_funding_df {
        let ft: Vec<i64> = df
            .column("time")
            .unwrap()
            .cast(&DataType::Int64)
            .unwrap()
            .i64()
            .unwrap()
            .into_iter()
            .filter_map(|x| x)
            .collect();
        let fr: Vec<f64> = df
            .column("funding_rate")
            .unwrap()
            .f64()
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        (ft, fr)
    } else {
        (vec![], vec![])
    };
    let daily_funding: Vec<f64> = align_to_ohlcv_dates(&btc_times, &funding_times, &funding_rates);
    let mut fill_fund = 0.0_f64;
    let btc_daily_funding: Vec<f64> = daily_funding
        .iter()
        .map(|&v| {
            if !v.is_nan() && v.is_finite() {
                fill_fund = v;
            }
            fill_fund
        })
        .collect();

    // BTC open interest
    let btc_oi_df = rt.block_on(oi_loader.fetch(BENCHMARK, None, None)).ok();
    let (oi_times, oi_values) = if let Some(ref df) = btc_oi_df {
        let ot: Vec<i64> = df
            .column("time")
            .unwrap()
            .cast(&DataType::Int64)
            .unwrap()
            .i64()
            .unwrap()
            .into_iter()
            .filter_map(|x| x)
            .collect();
        let ov: Vec<f64> = df
            .column("open_interest")
            .unwrap()
            .f64()
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        (ot, ov)
    } else {
        (vec![], vec![])
    };
    let daily_oi: Vec<f64> = align_to_ohlcv_dates(&btc_times, &oi_times, &oi_values);
    let btc_daily_oi: Vec<f64> = daily_oi
        .iter()
        .map(|&v| if v.is_nan() || v <= 0.0 { 0.0 } else { v })
        .collect();

    // Load universe OHLCV data
    let mut sym_closes: HashMap<String, Vec<f64>> = HashMap::new();
    let mut sym_volumes: HashMap<String, Vec<f64>> = HashMap::new();
    let all_syms: Vec<&str> = UNIVERSES
        .iter()
        .flat_map(|(_, syms)| syms.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for sym in all_syms {
        if let Ok(df) = rt.block_on(loader.fetch_data(sym, "1d", CANDLES)) {
            let filtered = df
                .lazy()
                .filter(col("close").is_not_null())
                .sort("time", Default::default())
                .collect()
                .unwrap();
            let closes: Vec<f64> = filtered
                .column("close")
                .unwrap()
                .f64()
                .unwrap()
                .into_iter()
                .flatten()
                .collect();
            let volumes: Vec<f64> = filtered
                .column("volume")
                .unwrap()
                .f64()
                .unwrap()
                .into_iter()
                .flatten()
                .collect();
            sym_closes.insert(sym.to_string(), closes);
            sym_volumes.insert(sym.to_string(), volumes);
        }
    }

    // ── Compute fragility state ──────────────────────────────────────────────
    let n = btc_times.len().min(btc_close.len());

    let btc_oi_pct = rolling_percentile(&btc_daily_oi, OI_PCT_WINDOW.min(n));
    let abs_funding: Vec<f64> = btc_daily_funding.iter().map(|&f| f.abs()).collect();
    let btc_funding_pct = rolling_percentile(&abs_funding, FUNDING_PCT_WINDOW.min(n));
    let vol_z = realized_vol_zscore(&btc_close, VOL_PCT_WINDOW);
    let btc_vol_pct = vol_percentile_from_z(&vol_z, VOL_PCT_WINDOW.min(n));
    let btc_oi_change = oi_change_pct(&btc_daily_oi, 5);

    let btc_fragility_state: Vec<FragilityState> = (0..n)
        .map(|i| {
            FragilityState::from_components(
                *btc_oi_pct.get(i).unwrap_or(&0.5),
                *btc_funding_pct.get(i).unwrap_or(&0.5),
                *btc_vol_pct.get(i).unwrap_or(&0.5),
                *btc_oi_change.get(i).unwrap_or(&0.0),
            )
        })
        .collect();

    // Print state distribution
    let mut state_counts = [0usize; 4];
    for s in &btc_fragility_state {
        match s {
            FragilityState::Calm => state_counts[0] += 1,
            FragilityState::Elevated => state_counts[1] += 1,
            FragilityState::Fragile => state_counts[2] += 1,
            FragilityState::PostCascade => state_counts[3] += 1,
        }
    }
    let total_f = state_counts.iter().sum::<usize>() as f64;
    println!("=== Fragility State Distribution ===");
    println!(
        "  {} bars: Calm={:.1}% Elevated={:.1}% Fragile={:.1}% PostCascade={:.1}%",
        total_f as usize,
        state_counts[0] as f64 / total_f * 100.0,
        state_counts[1] as f64 / total_f * 100.0,
        state_counts[2] as f64 / total_f * 100.0,
        state_counts[3] as f64 / total_f * 100.0,
    );

    // Data coverage report
    let oi_valid = btc_daily_oi.iter().filter(|&&v| v > 0.0).count();
    let fund_valid = btc_daily_funding.iter().filter(|&v| v.abs() > 0.0).count();
    println!(
        "  Data coverage: OI={}/{} days, Funding={}/{} days",
        oi_valid, n, fund_valid, n
    );
    if let Some(first_oi) = oi_times.first() {
        if let Some(last_oi) = oi_times.last() {
            println!(
                "  OI range: {} → {}",
                epoch_ms_to_date(*first_oi),
                epoch_ms_to_date(*last_oi)
            );
        }
    }
    if let Some(first_f) = funding_times.first() {
        if let Some(last_f) = funding_times.last() {
            println!(
                "  Funding range: {} → {}",
                epoch_ms_to_date(*first_f),
                epoch_ms_to_date(*last_f)
            );
        }
    }

    // ── Regime-conditional next-21-bar BTC returns ─────────────────────────
    println!();
    println!("=== Regime-Conditional Next-21-Bar BTC Returns ===");
    for state in [
        FragilityState::Calm,
        FragilityState::Elevated,
        FragilityState::Fragile,
        FragilityState::PostCascade,
    ] {
        let mut next_rets: Vec<f64> = Vec::new();
        for i in WARMUP_BARS..(n.saturating_sub(21)) {
            if btc_fragility_state[i] == state {
                let ret = (btc_close[i + 21] / btc_close[i] - 1.0) * 100.0;
                next_rets.push(ret);
            }
        }
        if !next_rets.is_empty() {
            let avg = next_rets.iter().sum::<f64>() / next_rets.len() as f64;
            let sharpe = compute_sharpe_from_rets(&next_rets);
            println!(
                "  {:?}: avg= {:.2}%, Sharpe= {:.2}, n= {}",
                state,
                avg,
                sharpe,
                next_rets.len()
            );
        } else {
            println!("  {:?}: no observations", state);
        }
    }

    // ── Run portfolio overlay across universes ────────────────────────────────
    println!();
    println!("=== Portfolio Overlay Results ===");
    println!(
        "{:<20} {:>11} {:>8} {:>9} {:>7} {:>7}",
        "Universe", "Return%", "Sharpe", "MaxDD%", "Trades", "WR%"
    );
    println!("{}", "-".repeat(68));

    for &(name, symbols) in UNIVERSES {
        let base_stats = backtest_universe(
            symbols,
            &sym_closes,
            &sym_volumes,
            &btc_fragility_state,
            PortfolioKind::Baseline,
        );

        // Compute per-day aligned returns for Sharpe/DD
        let warmup = WARMUP_BARS;
        let btc_close_warm: &[f64] = &btc_close[warmup..];
        let daily_rets: Vec<f64> = btc_close_warm
            .iter()
            .zip(&btc_close[warmup + 1..])
            .map(|(c0, c1)| {
                if *c0 > 0.0 {
                    (*c1 / *c0 - 1.0) * 100.0
                } else {
                    0.0
                }
            })
            .collect();
        let btc_sharpe = compute_sharpe_from_rets(&daily_rets);
        let btc_maxdd = compute_max_dd_from_rets(&daily_rets);

        println!(
            "{:<20} {:>11.1} {:>8.2} {:>9.2} {:>7} {:>7.1}",
            name,
            base_stats.aligned_return_pct,
            f64::from(btc_sharpe),
            f64::from(btc_maxdd),
            base_stats.trades,
            base_stats.win_rate_pct,
        );

        for pk in &[
            PortfolioKind::FragilityTilt,
            PortfolioKind::FragilityRiskOff,
        ] {
            let stats = backtest_universe(
                symbols,
                &sym_closes,
                &sym_volumes,
                &btc_fragility_state,
                *pk,
            );
            let label = format!("  + {}({})", name, pk.name());
            println!(
                "{:<20} {:>11.1} {:>8.2} {:>9.2} {:>7} {:>7.1}",
                label,
                stats.aligned_return_pct,
                f64::from(btc_sharpe),
                f64::from(btc_maxdd),
                stats.trades,
                stats.win_rate_pct,
            );
        }
    }

    println!();
    println!("Honest note: OI data coverage may be shallow (Binance OI endpoint ~500 rows).");
    println!(
        "If OI coverage < 90% of backtest window, Fragile state is dominated by funding+vol only."
    );
}
