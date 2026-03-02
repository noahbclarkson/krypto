use chrono::prelude::*;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use krypto::algo::optimization::{OptimizableStrategy, Optimizer};
use krypto::algo::strategies::{
    AdaptiveMaCrossover, AtrBreakout, BollingerReversion, DynamicTrend, LeadLagStrategy, MacdTrend,
    ObvTrend, PriceMomentum, RelativeStrengthStrat, RsiMeanReversion, VolatilitySqueeze,
};
use krypto::backtest::engine::Backtester;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use plotters::prelude::RangedDateTime;
use plotters::prelude::*;
use plotters::style::Color;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

const PLOT_OUTPUT: &str = "backtest_results.png";
const CACHE_DIR: &str = "examples/cache";
const TRAILING_SL: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.10;

#[derive(Debug, Serialize, Deserialize)]
struct CandleCache {
    time_ms: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

#[derive(Debug)]
struct TradeCandidate {
    symbol: String,
    interval: String,
    strategy_name: String,
    train_score: f64,
    kelly: f64,
    test_pnl_abs: f64,
    test_apr: f64,
    test_dd: f64,
    robustness: f64,
    days_in_test: f64,
    equity_curve: Vec<(NaiveDateTime, f64)>,
}

fn cache_path(symbol: &str, interval: &str, limit: u16) -> PathBuf {
    let filename = format!("{symbol}_{interval}_{limit}.bin");
    Path::new(CACHE_DIR).join(filename)
}

fn df_to_cache(df: &DataFrame) -> anyhow::Result<Vec<CandleCache>> {
    let times = df.column("time")?.datetime()?;
    let time_vals: Vec<Option<NaiveDateTime>> = times.as_datetime_iter().collect();
    let opens = df.column("open")?.f64()?;
    let highs = df.column("high")?.f64()?;
    let lows = df.column("low")?.f64()?;
    let closes = df.column("close")?.f64()?;
    let volumes = df.column("volume")?.f64()?;

    let mut out = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        let ts = time_vals
            .get(i)
            .and_then(|t| t.map(|x| x.and_utc().timestamp_millis()))
            .unwrap_or(0);
        out.push(CandleCache {
            time_ms: ts,
            open: opens.get(i).unwrap_or(0.0),
            high: highs.get(i).unwrap_or(0.0),
            low: lows.get(i).unwrap_or(0.0),
            close: closes.get(i).unwrap_or(0.0),
            volume: volumes.get(i).unwrap_or(0.0),
        });
    }
    Ok(out)
}

fn cache_to_df(records: Vec<CandleCache>) -> anyhow::Result<DataFrame> {
    let mut times = Vec::with_capacity(records.len());
    let mut opens = Vec::with_capacity(records.len());
    let mut highs = Vec::with_capacity(records.len());
    let mut lows = Vec::with_capacity(records.len());
    let mut closes = Vec::with_capacity(records.len());
    let mut volumes = Vec::with_capacity(records.len());

    for r in records {
        let ts = chrono::DateTime::from_timestamp_millis(r.time_ms)
            .map(|dt| dt.naive_utc())
            .unwrap_or_else(|| {
                chrono::DateTime::from_timestamp(0, 0)
                    .map(|dt| dt.naive_utc())
                    .unwrap_or(NaiveDateTime::MIN)
            });
        times.push(ts);
        opens.push(r.open);
        highs.push(r.high);
        lows.push(r.low);
        closes.push(r.close);
        volumes.push(r.volume);
    }

    let df = df!(
        "time" => times,
        "open" => opens,
        "high" => highs,
        "low" => lows,
        "close" => closes,
        "volume" => volumes
    )?;
    Ok(df)
}

async fn load_or_fetch(
    loader: &DataLoader,
    symbol: &str,
    interval: &str,
    limit: u16,
) -> anyhow::Result<(DataFrame, bool)> {
    let path = cache_path(symbol, interval, limit);
    if path.exists() {
        let file = BufReader::new(fs::File::open(path)?);
        let records: Vec<CandleCache> = bincode::deserialize_from(file)?;
        return cache_to_df(records).map(|df| (df, true));
    }

    let df = loader.fetch_data(symbol, interval, limit).await?;
    fs::create_dir_all(CACHE_DIR)?;
    let records = df_to_cache(&df)?;
    let file = BufWriter::new(fs::File::create(path)?);
    bincode::serialize_into(file, &records)?;
    Ok((df, false))
}

fn calculate_apr(total_return_pct: f64, days: f64) -> f64 {
    if days < 1.0 {
        return 0.0;
    }
    let r = total_return_pct / 100.0;
    ((1.0 + r).powf(365.0 / days) - 1.0) * 100.0
}

fn evaluate_strategy<S: OptimizableStrategy + Clone>(
    optimizer: &Optimizer,
    backtester: &Backtester,
    strat: &mut S,
    df: &DataFrame,
    test_df: &DataFrame,
    symbol: &str,
    interval: &str,
    candidates: &mut Vec<TradeCandidate>,
) -> anyhow::Result<()> {
    let (_, train_result) = optimizer.optimize(strat, df);

    if let Some(res) = train_result {
        if res.sharpe_ratio > 0.05 && res.profit_factor > 1.2 && res.total_trades > 20 {
            if let Ok(signals) = strat.predict(test_df) {
                if let Ok(test_res) = backtester.run(test_df, &signals, TRAILING_SL, TAKE_PROFIT) {
                    let times_ca = test_df.column("time")?.datetime()?;

                    let mut equity_curve = Vec::with_capacity(test_res.equity_curve.len());
                    for (i, t_opt) in times_ca.as_datetime_iter().enumerate() {
                        if let Some(t) = t_opt {
                            let val = test_res.equity_curve.get(i).copied().unwrap_or(10_000.0);
                            equity_curve.push((t, val));
                        }
                    }

                    let start_date = equity_curve
                        .first()
                        .map(|x| x.0)
                        .unwrap_or(NaiveDateTime::MIN);
                    let end_date = equity_curve
                        .last()
                        .map(|x| x.0)
                        .unwrap_or(NaiveDateTime::MAX);

                    let duration_days = (end_date - start_date).num_hours() as f64 / 24.0;
                    let apr = calculate_apr(test_res.total_return_pct, duration_days);
                    let adjusted_pnl = (test_res.final_equity - 10_000.0) * res.kelly_fraction;
                    let robustness = if res.sharpe_ratio.abs() > f64::EPSILON {
                        test_res.sharpe_ratio / res.sharpe_ratio
                    } else {
                        0.0
                    };

                    if robustness > 0.4 && test_res.total_trades > 30 {
                        candidates.push(TradeCandidate {
                            symbol: symbol.to_string(),
                            interval: interval.to_string(),
                            strategy_name: strat.name().to_string(),
                            train_score: res.sharpe_ratio,
                            kelly: res.kelly_fraction,
                            test_pnl_abs: adjusted_pnl,
                            test_apr: apr,
                            test_dd: test_res.max_drawdown_pct,
                            robustness,
                            days_in_test: duration_days,
                            equity_curve,
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

fn draw_chart(cands: &[TradeCandidate]) -> Result<(), Box<dyn std::error::Error>> {
    if cands.is_empty() {
        return Ok(());
    }

    let root = BitMapBackend::new(PLOT_OUTPUT, (1280, 720)).into_drawing_area();
    root.fill(&WHITE)?;

    let (min_date, max_date, y_min, y_max) = {
        let mut min_t = NaiveDateTime::MAX;
        let mut max_t = NaiveDateTime::MIN;
        let mut min_v = f64::MAX;
        let mut max_v = f64::MIN;

        for c in cands.iter().take(5) {
            for (t, v) in &c.equity_curve {
                if *t < min_t {
                    min_t = *t;
                }
                if *t > max_t {
                    max_t = *t;
                }
                if *v < min_v {
                    min_v = *v;
                }
                if *v > max_v {
                    max_v = *v;
                }
            }
        }
        let y_pad = (max_v - min_v).max(1.0) * 0.05;
        (min_t, max_t, min_v - y_pad, max_v + y_pad)
    };

    let x_spec = RangedDateTime::from(min_date..max_date);

    let mut chart = ChartBuilder::on(&root)
        .caption("Krypto V6: Top Portfolio Performance", ("sans-serif", 30))
        .margin(20)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(x_spec, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_label_formatter(&|dt| dt.format("%Y-%m-%d").to_string())
        .draw()?;

    let colors = [RED, BLUE, GREEN, MAGENTA, CYAN];
    for (i, cand) in cands.iter().take(5).enumerate() {
        let color = colors[i % colors.len()];
        chart
            .draw_series(LineSeries::new(cand.equity_curve.iter().copied(), color))?
            .label(format!("{} {}", cand.symbol, cand.strategy_name))
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    println!("Generated performance chart: {PLOT_OUTPUT}");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let symbols = vec!["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "DOGEFDUSD", "XRPFDUSD"];
    let intervals = vec!["1h", "4h", "1d"];
    let limit: u16 = 10_000;

    println!(
        "{}",
        "--- KRYPTO V6: INSTITUTIONAL ENGINE ---".green().bold()
    );
    println!("Fetching Deep History ({limit} candles/pair)...");

    let loader = DataLoader::new(None, None);
    let total_jobs = (symbols.len() * intervals.len()) as u64;
    let pb = ProgressBar::new(total_jobs);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )?
            .progress_chars("#>-"),
    );

    let mut data_map: HashMap<String, DataFrame> = HashMap::new();
    let mut cache_hits = 0usize;
    for sym in &symbols {
        for inv in &intervals {
            let key = format!("{sym}_{inv}");
            let (df, hit) = load_or_fetch(&loader, sym, inv, limit).await?;
            if hit {
                cache_hits += 1;
            }
            data_map.insert(key, df);
            pb.inc(1);
        }
    }
    pb.finish_with_message("Data ready");

    let mut btc_refs = HashMap::new();
    for inv in &intervals {
        let key = format!("BTCFDUSD_{inv}");
        if let Some(df) = data_map.get(&key) {
            let btc_tech = FeatureEngine::add_technicals(df, None)?;
            btc_refs.insert(inv.to_string(), btc_tech);
        }
    }

    let backtester = Backtester::new(10_000.0, 0.0, 0.001);
    let optimizer = Optimizer::new(160, 0.60);

    let mut candidates = Vec::new();

    for (key, raw_df) in &data_map {
        let parts: Vec<&str> = key.split('_').collect();
        if parts.len() != 2 {
            continue;
        }
        let symbol = parts[0];
        let interval = parts[1];

        let bench = btc_refs.get(interval);
        let df = match FeatureEngine::add_technicals(raw_df, bench) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let len = df.height();
        let train_len = (len as f64 * 0.6) as usize;
        let test_df = df.slice(train_len as i64, len - train_len);

        // 1. Dynamic Trend
        let mut strat_trend = DynamicTrend::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_trend,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 2. ATR Breakout
        let mut strat_atr = AtrBreakout::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_atr,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 3. Relative Strength (non-BTC)
        if !symbol.contains("BTC") {
            let mut strat_rs = RelativeStrengthStrat::new();
            evaluate_strategy(
                &optimizer,
                &backtester,
                &mut strat_rs,
                &df,
                &test_df,
                symbol,
                interval,
                &mut candidates,
            )?;
        }

        // 4. Bollinger Reversion
        let mut strat_bb = BollingerReversion::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_bb,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 5. Volatility Squeeze
        let mut strat_sq = VolatilitySqueeze::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_sq,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 6. Lead-Lag (non-BTC)
        if !symbol.contains("BTC") {
            let mut strat_lead = LeadLagStrategy::new();
            evaluate_strategy(
                &optimizer,
                &backtester,
                &mut strat_lead,
                &df,
                &test_df,
                symbol,
                interval,
                &mut candidates,
            )?;
        }

        // --- NEW STRATEGIES ---

        // 7. OBV Trend (Volume Based)
        let mut strat_obv = ObvTrend::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_obv,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 8. MACD Trend (Momentum)
        let mut strat_macd = MacdTrend::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_macd,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 9. RSI Mean Reversion (Oscillator)
        let mut strat_rsi_rev = RsiMeanReversion::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_rsi_rev,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 10. Price Momentum (ROC)
        let mut strat_mom = PriceMomentum::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_mom,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;

        // 11. Adaptive MA Crossover
        let mut strat_ma = AdaptiveMaCrossover::new();
        evaluate_strategy(
            &optimizer,
            &backtester,
            &mut strat_ma,
            &df,
            &test_df,
            symbol,
            interval,
            &mut candidates,
        )?;
    }

    candidates.retain(|c| c.test_pnl_abs > 0.0);
    candidates.sort_by(|a, b| {
        b.train_score
            .partial_cmp(&a.train_score)
            .unwrap_or(Ordering::Equal)
    });

    println!(
        "\n{:<12} {:<6} {:<20} {:<6} {:<6} {:<10} {:<10} {:<8} {:<8} {:<6}",
        "Symbol", "Intv", "Strategy", "Kelly", "Score", "Test PnL", "APR %", "DD %", "Days", "Rob."
    );
    println!("{}", "-".repeat(110));

    let mut total_weighted_pnl = 0.0;
    let mut allocated_capital = 0.0;
    let top_n = 7;

    for cand in candidates.iter().take(top_n) {
        let pnl_color = if cand.test_pnl_abs > 0.0 {
            "green"
        } else {
            "red"
        };

        println!(
            "{:<12} {:<6} {:<20} {:<6.2} {:<6.2} ${:<9} {:<10.2} {:<8.2} {:<8.1} {:<6.2}",
            cand.symbol,
            cand.interval,
            cand.strategy_name,
            cand.kelly,
            cand.train_score,
            format!("{:.0}", cand.test_pnl_abs).color(pnl_color),
            cand.test_apr,
            cand.test_dd,
            cand.days_in_test,
            cand.robustness
        );

        total_weighted_pnl += cand.test_pnl_abs;
        allocated_capital += cand.kelly * 10_000.0;
    }

    println!("{}", "-".repeat(110));
    println!("Total Portfolio PnL (Risk Adjusted): ${total_weighted_pnl:.2}");
    println!("Allocated Capital (Kelly-weighted, nominal): ${allocated_capital:.2}");
    println!(
        "Cache hits: {}/{}",
        cache_hits,
        symbols.len() * intervals.len()
    );

    if !candidates.is_empty() {
        match std::panic::catch_unwind(|| draw_chart(&candidates)) {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => eprintln!("Chart rendering error: {e}"),
            Err(_) => {
                eprintln!("Chart rendering failed (font backend unavailable), skipping plot.")
            }
        }
    }

    Ok(())
}
