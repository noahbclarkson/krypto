//! Audit whether borderline OldGuardNoBNB rows survive a true passive-entry fill model.
//!
//! Why this exists:
//! - the fee-surface audit showed some old-guard rows crossing chronology thresholds
//!   under cheaper maker-style fees
//! - but cheaper fees alone are not enough; we need to ask whether those rows still
//!   survive when entry requires an actual passive fill within the next daily bar
//! - keep the same fair daily fixed-hold harness otherwise: signal at close,
//!   fixed 21-bar exit at daily open, no stop-path assumptions

use anyhow::{anyhow, Result};
use chrono::Utc;
use krypto::{
    algo::{strategies::CrossSectionalMomentum, SignalGenerator},
    backtest::passive::{
        bars_per_signal, lower_interval_for_signal, PassiveConfig, PassiveExecutor, TickSize,
    },
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const DAILY_INTERVAL: &str = "1d";
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const RESAMPLE_BLOCKS: usize = 6;
const TURTLE_PERIOD: usize = 20;
const CS_LOOKBACK: usize = 63;
const MIN_TRADES_PER_WINDOW: usize = 30;
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/oldguard_passive_entry_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/oldguard_passive_entry_latest.csv";

const UNIVERSE_LABEL: &str = "OldGuardNoBNB";
const SYMBOLS: &[&str] = &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    CrossSectionalMomentum,
    MacdRegime,
    EnsembleMajority3,
}

impl StrategyKind {
    fn all() -> &'static [StrategyKind] {
        &[
            Self::CrossSectionalMomentum,
            Self::MacdRegime,
            Self::EnsembleMajority3,
        ]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CrossSectionalMomentum => "CrossSectionalMomentum",
            Self::MacdRegime => "MACD+Regime",
            Self::EnsembleMajority3 => "Ensemble(Majority 2/3)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ExecutionScenario {
    TakerTaker,
    PassiveEntryTakerExit,
}

impl ExecutionScenario {
    fn all() -> &'static [ExecutionScenario] {
        &[Self::TakerTaker, Self::PassiveEntryTakerExit]
    }

    fn name(&self) -> &'static str {
        match self {
            Self::TakerTaker => "Taker/Taker (10+10 bps)",
            Self::PassiveEntryTakerExit => "PassiveEntry/TakerExit (real fill, 2+10 bps)",
        }
    }
}

#[derive(Clone, Debug)]
struct PreparedSymbol {
    daily: DataFrame,
    signals: HashMap<StrategyKind, Vec<i32>>,
    passive_fills: HashMap<StrategyKind, HashMap<usize, krypto::backtest::passive::FillEvent>>,
}

#[derive(Clone, Debug, Default)]
struct StrategyResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    attempted_trades: usize,
    filled_trades: usize,
    fill_bars_sum: usize,
    entry_edge_sum_bps: f64,
}

impl StrategyResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }

    fn fill_rate(&self) -> f64 {
        if self.attempted_trades == 0 {
            0.0
        } else {
            self.filled_trades as f64 / self.attempted_trades as f64
        }
    }

    fn avg_fill_bars(&self) -> f64 {
        if self.filled_trades == 0 {
            0.0
        } else {
            self.fill_bars_sum as f64 / self.filled_trades as f64
        }
    }

    fn avg_entry_edge_bps(&self) -> f64 {
        if self.filled_trades == 0 {
            0.0
        } else {
            self.entry_edge_sum_bps / self.filled_trades as f64
        }
    }

    fn add_assign(&mut self, other: &StrategyResult) {
        self.total_return_pct += other.total_return_pct;
        self.trades += other.trades;
        self.wins += other.wins;
        self.attempted_trades += other.attempted_trades;
        self.filled_trades += other.filled_trades;
        self.fill_bars_sum += other.fill_bars_sum;
        self.entry_edge_sum_bps += other.entry_edge_sum_bps;
    }
}

#[derive(Clone, Debug)]
struct ResultRow {
    strategy: StrategyKind,
    scenario: ExecutionScenario,
    full_return_pct: f64,
    full_trades: usize,
    attempted_trades: usize,
    fill_rate_pct: f64,
    avg_fill_bars: f64,
    avg_entry_edge_bps: f64,
    win_rate_pct: f64,
    wf_passed: usize,
    wf_total: usize,
    resample_passed: usize,
    resample_total: usize,
    wf_avg_return_pct: f64,
    resample_avg_return_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== OLDGUARD PASSIVE ENTRY AUDIT ===\n");
    println!("Universe: {} ({})", UNIVERSE_LABEL, SYMBOLS.join(", "));
    println!("Goal: test whether borderline old-guard rows still survive when maker-style entry requires a real passive fill");
    println!(
        "Execution baseline: signal at close, next-open entry, fixed {}-bar exit at daily open",
        HOLD_BARS
    );
    println!("Passive scenario: anchored passive entry inside the next daily bar using 30m candles, then same fixed daily exit\n");

    let loader = DataLoader::new(None, None);
    let lower_interval = lower_interval_for_signal(DAILY_INTERVAL);
    let lower_bars_per_signal = bars_per_signal(DAILY_INTERVAL);
    let lower_candles = CANDLES * lower_bars_per_signal as u32;

    let raw_bench = loader
        .fetch_with_cache("BTCUSDT", DAILY_INTERVAL, CANDLES)
        .await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    let mut daily_cache = HashMap::<String, DataFrame>::new();
    daily_cache.insert("BTCUSDT".to_string(), bench_df.clone());
    for &symbol in SYMBOLS {
        print!("Loading daily {}... ", symbol);
        let raw = loader
            .fetch_with_cache(symbol, DAILY_INTERVAL, CANDLES)
            .await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        daily_cache.insert(symbol.to_string(), enriched);
    }

    let mut cs_map = HashMap::<String, DataFrame>::new();
    for &symbol in SYMBOLS {
        cs_map.insert(symbol.to_string(), daily_cache.get(symbol).unwrap().clone());
    }
    compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;
    for (symbol, df) in cs_map {
        daily_cache.insert(symbol, df);
    }

    let mut low_cache = HashMap::<String, DataFrame>::new();
    for &symbol in SYMBOLS {
        print!("Loading {} {} fills... ", symbol, lower_interval);
        let df_low = loader
            .fetch_with_cache(symbol, lower_interval, lower_candles)
            .await?;
        println!("{} bars", df_low.height());
        low_cache.insert(symbol.to_string(), df_low);
    }

    let prepared =
        prepare_symbol_data(&daily_cache, &low_cache, SYMBOLS, lower_bars_per_signal).await?;
    let quarter_windows = quarter_windows(&prepared, SYMBOLS)?;
    let resample_sets = cpcv_style_windows(&prepared, SYMBOLS)?;
    let mut rows = Vec::new();

    for &strategy in StrategyKind::all() {
        for &scenario in ExecutionScenario::all() {
            let full = evaluate_strategy_on_windows(
                &prepared,
                strategy,
                SYMBOLS,
                &[(0, usable_len(&prepared, SYMBOLS)?)],
                scenario,
            )?;

            let mut wf_passed = 0usize;
            let mut wf_return_sum = 0.0;
            for &(start, end) in &quarter_windows {
                let eval = evaluate_strategy_on_windows(
                    &prepared,
                    strategy,
                    SYMBOLS,
                    &[(start, end)],
                    scenario,
                )?;
                wf_return_sum += eval.total_return_pct;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    wf_passed += 1;
                }
            }

            let mut resample_passed = 0usize;
            let mut resample_return_sum = 0.0;
            for window_set in &resample_sets {
                let eval = evaluate_strategy_on_windows(
                    &prepared, strategy, SYMBOLS, window_set, scenario,
                )?;
                resample_return_sum += eval.total_return_pct;
                if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                    resample_passed += 1;
                }
            }

            rows.push(ResultRow {
                strategy,
                scenario,
                full_return_pct: full.total_return_pct,
                full_trades: full.trades,
                attempted_trades: full.attempted_trades,
                fill_rate_pct: full.fill_rate() * 100.0,
                avg_fill_bars: full.avg_fill_bars(),
                avg_entry_edge_bps: full.avg_entry_edge_bps(),
                win_rate_pct: full.win_rate() * 100.0,
                wf_passed,
                wf_total: quarter_windows.len(),
                resample_passed,
                resample_total: resample_sets.len(),
                wf_avg_return_pct: wf_return_sum / quarter_windows.len() as f64,
                resample_avg_return_pct: resample_return_sum / resample_sets.len() as f64,
            });
        }
    }

    rows.sort_by(|a, b| {
        b.resample_passed
            .cmp(&a.resample_passed)
            .then_with(|| b.wf_passed.cmp(&a.wf_passed))
            .then_with(|| b.full_return_pct.partial_cmp(&a.full_return_pct).unwrap())
    });

    println!(
        "\n{:<28} {:<38} {:>10} {:>8} {:>8} {:>8} {:>10} {:>9} {:>8}",
        "Strategy",
        "Scenario",
        "FullRet%",
        "Trades",
        "WF",
        "RS",
        "FillRate",
        "Edge(bps)",
        "WinRate"
    );
    println!("{}", "-".repeat(138));
    for row in &rows {
        println!(
            "{:<28} {:<38} {:>9.1} {:>8} {:>4}/{} {:>4}/{} {:>9.1}% {:>9.1} {:>7.1}%",
            row.strategy.name(),
            row.scenario.name(),
            row.full_return_pct,
            row.full_trades,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
            row.fill_rate_pct,
            row.avg_entry_edge_bps,
            row.win_rate_pct,
        );
    }

    let (archive_md, archive_csv) = write_snapshot(&rows)?;
    println!("\nSnapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);
    println!("\nInterpretation:");
    println!("- If a borderline row keeps or improves chronology under real passive fills, the maker-style story is more credible.");
    println!("- If chronology gains disappear once skipped fills are modeled, the fee-surface uplift was mostly accounting, not execution realism.");

    Ok(())
}

async fn prepare_symbol_data(
    daily_cache: &HashMap<String, DataFrame>,
    low_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    lower_bars_per_signal: usize,
) -> Result<HashMap<String, PreparedSymbol>> {
    let mut prepared = HashMap::new();
    for &symbol in symbols {
        let daily = daily_cache
            .get(symbol)
            .ok_or_else(|| anyhow!("missing {}", symbol))?
            .clone();
        let low = low_cache
            .get(symbol)
            .ok_or_else(|| anyhow!("missing low {}", symbol))?
            .clone();

        let mut strategy_signals = HashMap::new();
        for &strategy in StrategyKind::all() {
            strategy_signals.insert(strategy, signal_for_strategy(&daily, strategy)?);
        }

        let tick = TickSize::fetch(symbol)
            .await
            .unwrap_or_else(|_| fallback_tick_size(symbol));
        let passive_cfg = PassiveConfig {
            ticks_below_open: 3,
            tick_size: tick,
            max_wait_bars: lower_bars_per_signal,
            maker_fee: 0.0002,
            update_threshold_ticks: None,
            anchor_to_signal: true,
        };
        let executor = PassiveExecutor::new(passive_cfg);

        let mut passive_fills = HashMap::new();
        for (&strategy, signals) in &strategy_signals {
            let shifted_signal_values: Vec<f64> = (0..daily.height())
                .map(|i| if i == 0 { 0.0 } else { signals[i - 1] as f64 })
                .collect();
            let shifted_signal_series = Series::new("signal", shifted_signal_values);
            let (fills, _stats) = executor
                .simulate(&daily, &low, &shifted_signal_series)
                .await?;
            passive_fills.insert(
                strategy,
                fills.into_iter().map(|f| (f.signal_bar, f)).collect(),
            );
        }

        prepared.insert(
            symbol.to_string(),
            PreparedSymbol {
                daily,
                signals: strategy_signals,
                passive_fills,
            },
        );
    }
    Ok(prepared)
}

fn evaluate_strategy_on_windows(
    prepared: &HashMap<String, PreparedSymbol>,
    strategy: StrategyKind,
    symbols: &[&str],
    windows: &[(usize, usize)],
    scenario: ExecutionScenario,
) -> Result<StrategyResult> {
    let mut out = StrategyResult::default();
    for &symbol in symbols {
        let prepared_symbol = prepared
            .get(symbol)
            .ok_or_else(|| anyhow!("missing prepared {}", symbol))?;
        let signals = prepared_symbol
            .signals
            .get(&strategy)
            .ok_or_else(|| anyhow!("missing signals {}", symbol))?;
        let fills = prepared_symbol
            .passive_fills
            .get(&strategy)
            .ok_or_else(|| anyhow!("missing fills {}", symbol))?;
        for &(start, end) in windows {
            let result =
                backtest_window(&prepared_symbol.daily, signals, fills, start, end, scenario)?;
            out.add_assign(&result);
        }
    }
    Ok(out)
}

fn signal_for_strategy(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    let macd_regime = generate_macd_regime_signals(df)?;
    let turtle_regime_macd = generate_turtle_regime_macd_signals(df, TURTLE_PERIOD)?;
    let cross_sectional = CrossSectionalMomentum::new()
        .predict(df)?
        .f64()?
        .into_iter()
        .map(|v| match v.unwrap_or(0.0).partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Less) => -1,
            _ => 0,
        })
        .collect::<Vec<_>>();

    Ok(match strategy {
        StrategyKind::CrossSectionalMomentum => cross_sectional,
        StrategyKind::MacdRegime => macd_regime,
        StrategyKind::EnsembleMajority3 => {
            consensus_signal(&[&macd_regime, &turtle_regime_macd, &cross_sectional], 2)
        }
    })
}

fn consensus_signal(signals: &[&Vec<i32>], min_agree: usize) -> Vec<i32> {
    let len = signals.first().map(|s| s.len()).unwrap_or(0);
    let mut out = vec![0i32; len];
    for i in 0..len {
        let long_votes = signals.iter().filter(|sig| sig[i] > 0).count();
        let short_votes = signals.iter().filter(|sig| sig[i] < 0).count();
        if long_votes >= min_agree {
            out[i] = 1;
        } else if short_votes >= min_agree {
            out[i] = -1;
        }
    }
    out
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];

    for i in 0..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let macd_now = macd.get(i).unwrap_or(0.0);
        let macd_sig_now = macd_signal.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        if macd_now > macd_sig_now && price > sma_now {
            out[i] = 1;
        } else if macd_now < macd_sig_now && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = df.column("macd")?.f64()?;
    let macd_signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0i32; df.height()];
    for i in 0..df.height() {
        let t = turtle[i];
        let m = if macd.get(i).unwrap_or(0.0) > macd_signal.get(i).unwrap_or(0.0) {
            1
        } else if macd.get(i).unwrap_or(0.0) < macd_signal.get(i).unwrap_or(0.0) {
            -1
        } else {
            0
        };
        if t == m {
            out[i] = t;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; df.height()];
    for i in period..df.height() {
        let price = close.get(i).unwrap_or(0.0);
        let sma_now = sma_200.get(i).copied().unwrap_or(0.0);
        if sma_now <= 0.0 {
            continue;
        }
        let highest = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        if price > highest && price > sma_now {
            out[i] = 1;
        } else if price < lowest && price < sma_now {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(values: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= values.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn backtest_window(
    df: &DataFrame,
    signals: &[i32],
    fills: &HashMap<usize, krypto::backtest::passive::FillEvent>,
    start: usize,
    end: usize,
    scenario: ExecutionScenario,
) -> Result<StrategyResult> {
    let open = df.column("open")?.f64()?;
    let mut out = StrategyResult::default();

    let mut i = start.max(1);
    while i + HOLD_BARS < end && i + HOLD_BARS < df.height() {
        let signal = signals[i - 1];
        if signal == 0 {
            i += 1;
            continue;
        }

        out.attempted_trades += 1;
        let exit = open.get(i + HOLD_BARS).unwrap_or(0.0);
        if exit <= 0.0 {
            i += 1;
            continue;
        }

        let (entry, total_fee, bars_to_fill, edge_bps_opt) = match scenario {
            ExecutionScenario::TakerTaker => {
                let entry = open.get(i).unwrap_or(0.0);
                (entry, 0.001 + 0.001, 0usize, None)
            }
            ExecutionScenario::PassiveEntryTakerExit => {
                let Some(fill) = fills.get(&i) else {
                    i += HOLD_BARS;
                    continue;
                };
                let daily_open = open.get(i).unwrap_or(0.0);
                let edge_bps = if signal > 0 {
                    ((daily_open - fill.fill_price) / daily_open) * 10_000.0
                } else {
                    ((fill.fill_price - daily_open) / daily_open) * 10_000.0
                };
                (
                    fill.fill_price,
                    0.0002 + 0.001,
                    fill.bars_to_fill,
                    Some(edge_bps),
                )
            }
        };

        if entry <= 0.0 {
            i += 1;
            continue;
        }

        let gross = if signal > 0 {
            (exit / entry) - 1.0
        } else {
            (entry / exit) - 1.0
        };
        let net = gross - total_fee;
        out.total_return_pct += net * 100.0;
        out.trades += 1;
        out.filled_trades += 1;
        out.fill_bars_sum += bars_to_fill;
        if let Some(edge_bps) = edge_bps_opt {
            out.entry_edge_sum_bps += edge_bps;
        }
        if net > 0.0 {
            out.wins += 1;
        }
        i += HOLD_BARS;
    }

    Ok(out)
}

fn fallback_tick_size(symbol: &str) -> TickSize {
    if symbol.contains("BTC") {
        TickSize::from_value(0.1)
    } else if symbol.contains("ETH") {
        TickSize::from_value(0.01)
    } else {
        TickSize::from_value(0.001)
    }
}

fn usable_len(prepared: &HashMap<String, PreparedSymbol>, symbols: &[&str]) -> Result<usize> {
    Ok(min_symbol_len(prepared, symbols)?.saturating_sub(HOLD_BARS + 1))
}

fn min_symbol_len(prepared: &HashMap<String, PreparedSymbol>, symbols: &[&str]) -> Result<usize> {
    symbols
        .iter()
        .filter_map(|symbol| prepared.get(*symbol).map(|p| p.daily.height()))
        .min()
        .ok_or_else(|| anyhow!("empty universe"))
}

fn quarter_windows(
    prepared: &HashMap<String, PreparedSymbol>,
    symbols: &[&str],
) -> Result<Vec<(usize, usize)>> {
    let usable = usable_len(prepared, symbols)?;
    let quarter = usable / 4;
    let mut out = Vec::new();
    for k in 0..4 {
        let start = k * quarter;
        let end = if k == 3 { usable } else { (k + 1) * quarter };
        out.push((start, end));
    }
    Ok(out)
}

fn cpcv_style_windows(
    prepared: &HashMap<String, PreparedSymbol>,
    symbols: &[&str],
) -> Result<Vec<Vec<(usize, usize)>>> {
    let usable = usable_len(prepared, symbols)?;
    let block = usable / RESAMPLE_BLOCKS;
    let ranges: Vec<(usize, usize)> = (0..RESAMPLE_BLOCKS)
        .map(|i| {
            let start = i * block;
            let end = if i == RESAMPLE_BLOCKS - 1 {
                usable
            } else {
                (i + 1) * block
            };
            (start, end)
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..RESAMPLE_BLOCKS {
        for j in i + 1..RESAMPLE_BLOCKS {
            out.push(vec![ranges[i], ranges[j]]);
        }
    }
    Ok(out)
}

fn write_snapshot(rows: &[ResultRow]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/oldguard_passive_entry_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/oldguard_passive_entry_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# OldGuard Passive Entry Audit Snapshot\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!(
        "- Universe: {} ({})\n",
        UNIVERSE_LABEL,
        SYMBOLS.join(", ")
    ));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(
        "- Baseline: signal at close, next-open taker entry, fixed-hold exit at daily open\n",
    );
    markdown.push_str("- Passive scenario: real 30m passive-entry fill inside next daily bar, then same fixed-hold daily exit\n\n");
    markdown.push_str("| Strategy | Scenario | Full Return % | Trades | Attempted | Fill Rate % | Avg Fill Bars | Avg Entry Edge bps | Win Rate % | WF | RS | WF AvgRet % | RS AvgRet % |\n");
    markdown.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");

    let mut csv = String::from("strategy,scenario,full_return_pct,full_trades,attempted_trades,fill_rate_pct,avg_fill_bars,avg_entry_edge_bps,win_rate_pct,wf_passed,wf_total,resample_passed,resample_total,wf_avg_return_pct,resample_avg_return_pct\n");

    for row in rows {
        markdown.push_str(&format!(
            "| {} | {} | {:.1} | {} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {}/{} | {}/{} | {:.1} | {:.1} |\n",
            row.strategy.name(), row.scenario.name(), row.full_return_pct, row.full_trades,
            row.attempted_trades, row.fill_rate_pct, row.avg_fill_bars, row.avg_entry_edge_bps,
            row.win_rate_pct, row.wf_passed, row.wf_total, row.resample_passed, row.resample_total,
            row.wf_avg_return_pct, row.resample_avg_return_pct
        ));
        csv.push_str(&format!(
            "{},{},{:.4},{},{},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{:.4},{:.4}\n",
            row.strategy.name(),
            row.scenario.name(),
            row.full_return_pct,
            row.full_trades,
            row.attempted_trades,
            row.fill_rate_pct,
            row.avg_fill_bars,
            row.avg_entry_edge_bps,
            row.win_rate_pct,
            row.wf_passed,
            row.wf_total,
            row.resample_passed,
            row.resample_total,
            row.wf_avg_return_pct,
            row.resample_avg_return_pct
        ));
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;
    Ok((archive_md, archive_csv))
}
