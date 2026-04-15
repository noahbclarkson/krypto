use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs;

const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const TRAIN_BARS: usize = 252;
const TOP_K: usize = 3;
const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
];

#[derive(Clone)]
struct TradeSignal {
    strength: f64,
    ret_pct: f64,
}

fn main() -> Result<()> {
    fs::create_dir_all("charts")?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();

    let all_symbols = [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
    ];
    for s in all_symbols {
        let raw = loader.fetch_data(s, "1d", 3000).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        cache.insert(s.to_string(), df);
    }

    for (uname, symbols) in UNIVERSES {
        let chart_prefix = format!("charts/linear_model_vs_leaders_{}", uname.to_lowercase());
        let curves = build_curves(&cache, symbols)?;
        draw_equity_chart(&format!("{}_equity.png", chart_prefix), uname, &curves)?;
        draw_drawdown_chart(&format!("{}_drawdown.png", chart_prefix), uname, &curves)?;
    }

    Ok(())
}

fn build_curves(
    cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<Vec<(String, Vec<f64>)>> {
    let names = vec![
        "LinearAlpha(panel)",
        "MACD+Regime",
        "Turtle+Regime+MACD",
        "Ensemble(Majority 2/3)",
    ];
    let mut all: Vec<(String, Vec<f64>)> = Vec::new();
    for name in names {
        let curve = simulate_portfolio_curve(cache, symbols, name)?;
        all.push((name.to_string(), curve));
    }
    Ok(all)
}

fn simulate_portfolio_curve(
    cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
    strategy: &str,
) -> Result<Vec<f64>> {
    let n = symbols
        .iter()
        .filter_map(|s| cache.get(&s.to_string()).map(|df| df.height()))
        .min()
        .unwrap_or(0);
    let start = TRAIN_BARS.max(200);
    let mut daily_signals: Vec<Vec<TradeSignal>> = vec![Vec::new(); n];

    for &sym in symbols {
        let df = cache.get(&sym.to_string()).unwrap();
        let close = df.column("close")?.f64()?;
        let highs = df.column("high")?.f64()?;
        let lows = df.column("low")?.f64()?;
        let sma200 = calc_sma(close, 200);
        let macd = calc_macd(close)?;
        let turtle = calc_turtle(close, highs, lows, 20);
        let csm = calc_csm_signal(close);
        let lin = calc_linear_score(close, &macd, &sma200, TRAIN_BARS);

        let len = n.min(close.len());
        for t in start..len.saturating_sub(HOLD_BARS + 1) {
            let entry = close.get(t + 1).unwrap_or(0.0);
            let exit = close.get(t + 1 + HOLD_BARS).unwrap_or(0.0);
            if entry <= 0.0 || exit <= 0.0 {
                continue;
            }
            let mut dir = 0i32;
            let mut strength = 0.0;
            match strategy {
                "MACD+Regime" => {
                    if macd[t] > 0.0 && close.get(t).unwrap_or(0.0) > sma200[t] {
                        dir = 1;
                        strength = macd[t].abs();
                    }
                }
                "Turtle+Regime+MACD" => {
                    if turtle[t] > 0 && macd[t] > 0.0 && close.get(t).unwrap_or(0.0) > sma200[t] {
                        dir = 1;
                        strength = 1.0 + macd[t].abs();
                    }
                }
                "Ensemble(Majority 2/3)" => {
                    let v1 = if macd[t] > 0.0 && close.get(t).unwrap_or(0.0) > sma200[t] {
                        1
                    } else {
                        0
                    };
                    let v2 = if turtle[t] > 0
                        && macd[t] > 0.0
                        && close.get(t).unwrap_or(0.0) > sma200[t]
                    {
                        1
                    } else {
                        0
                    };
                    let v3 = if csm[t] > 0.0 { 1 } else { 0 };
                    if v1 + v2 + v3 >= 2 {
                        dir = 1;
                        strength = (v1 + v2 + v3) as f64;
                    }
                }
                "LinearAlpha(panel)" => {
                    if lin[t] > 0.0 {
                        dir = 1;
                        strength = lin[t];
                    }
                }
                _ => {}
            }
            if dir != 0 {
                let gross = (exit / entry - 1.0) * 100.0 * dir as f64;
                let net = gross - 2.0 * TAKER_FEE * 100.0;
                daily_signals[t].push(TradeSignal {
                    strength,
                    ret_pct: net,
                });
            }
        }
    }

    let mut equity = 100.0;
    let mut curve = Vec::new();
    for signals in daily_signals.iter().skip(start) {
        let mut picks = signals.clone();
        picks.sort_by(|a, b| {
            b.strength
                .partial_cmp(&a.strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let active = picks.into_iter().take(TOP_K).collect::<Vec<_>>();
        if active.is_empty() {
            curve.push(equity);
            continue;
        }
        let avg_ret = active.iter().map(|x| x.ret_pct).sum::<f64>() / active.len() as f64;
        equity *= 1.0 + avg_ret / 100.0;
        curve.push(equity);
    }
    Ok(curve)
}

fn draw_equity_chart(path: &str, universe: &str, curves: &[(String, Vec<f64>)]) -> Result<()> {
    let root = BitMapBackend::new(path, (1400, 900)).into_drawing_area();
    root.fill(&WHITE)?;
    let max_x = curves.iter().map(|(_, c)| c.len()).max().unwrap_or(1);
    let max_y = curves
        .iter()
        .flat_map(|(_, c)| c.iter().copied())
        .fold(100.0_f64, f64::max)
        * 1.05;
    let min_y = 1.0_f64;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Equity curves (log scale): Linear model vs current leaders ({})",
                universe
            ),
            ("sans-serif", 30).into_font(),
        )
        .margin(24)
        .x_label_area_size(40)
        .y_label_area_size(90)
        .build_cartesian_2d(0..max_x, (min_y..max_y).log_scale())?;
    chart
        .configure_mesh()
        .x_desc("Bars")
        .y_desc("Equity (log scale, start = 1.0)")
        .light_line_style(TRANSPARENT)
        .bold_line_style(BLACK.mix(0.15))
        .axis_desc_style(("sans-serif", 20))
        .label_style(("sans-serif", 16))
        .y_label_formatter(&|v| {
            if *v >= 1_000_000.0 {
                format!("{:.0}M", v / 1_000_000.0)
            } else if *v >= 1_000.0 {
                format!("{:.0}k", v / 1_000.0)
            } else if *v >= 10.0 {
                format!("{:.0}", v)
            } else {
                format!("{:.1}", v)
            }
        })
        .draw()?;
    let colors = [
        RGBColor(31, 119, 180),
        RGBColor(255, 127, 14),
        RGBColor(44, 160, 44),
        RGBColor(214, 39, 40),
    ];
    for (i, (name, curve)) in curves.iter().enumerate() {
        chart
            .draw_series(LineSeries::new(
                curve.iter().enumerate().map(|(x, y)| (x, (*y).max(1.0))),
                colors[i].stroke_width(3),
            ))?
            .label(name.clone())
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], colors[i].stroke_width(3))
            });
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.88))
        .border_style(BLACK.mix(0.3))
        .label_font(("sans-serif", 18))
        .position(SeriesLabelPosition::UpperLeft)
        .draw()?;
    root.present()?;
    Ok(())
}

fn draw_drawdown_chart(path: &str, universe: &str, curves: &[(String, Vec<f64>)]) -> Result<()> {
    let dd_curves: Vec<(String, Vec<f64>)> = curves
        .iter()
        .map(|(n, c)| {
            let mut peak: f64 = 0.0;
            let mut dd = Vec::with_capacity(c.len());
            for &v in c {
                peak = peak.max(v);
                dd.push(if peak > 0.0 {
                    (peak - v) / peak * 100.0
                } else {
                    0.0
                });
            }
            (n.clone(), dd)
        })
        .collect();
    let root = BitMapBackend::new(path, (1400, 900)).into_drawing_area();
    root.fill(&WHITE)?;
    let max_x = dd_curves.iter().map(|(_, c)| c.len()).max().unwrap_or(1);
    let max_y = dd_curves
        .iter()
        .flat_map(|(_, c)| c.iter().copied())
        .fold(0.0_f64, f64::max)
        * 1.05
        + 1.0;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Drawdown curves: Linear model vs current leaders ({})",
                universe
            ),
            ("sans-serif", 30).into_font(),
        )
        .margin(24)
        .x_label_area_size(40)
        .y_label_area_size(80)
        .build_cartesian_2d(0..max_x, 0.0..max_y)?;
    chart
        .configure_mesh()
        .x_desc("Bars")
        .y_desc("Drawdown %")
        .light_line_style(TRANSPARENT)
        .bold_line_style(BLACK.mix(0.15))
        .axis_desc_style(("sans-serif", 20))
        .label_style(("sans-serif", 16))
        .draw()?;
    let colors = [
        RGBColor(31, 119, 180),
        RGBColor(255, 127, 14),
        RGBColor(44, 160, 44),
        RGBColor(214, 39, 40),
    ];
    for (i, (name, curve)) in dd_curves.iter().enumerate() {
        chart
            .draw_series(LineSeries::new(
                curve.iter().enumerate().map(|(x, y)| (x, *y)),
                colors[i].stroke_width(3),
            ))?
            .label(name.clone())
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], colors[i].stroke_width(3))
            });
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.88))
        .border_style(BLACK.mix(0.3))
        .label_font(("sans-serif", 18))
        .position(SeriesLabelPosition::UpperLeft)
        .draw()?;
    root.present()?;
    Ok(())
}

fn calc_sma(close: &Float64Chunked, period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in period..n {
        let mut s = 0.0;
        let mut c = 0;
        for j in (i - period + 1)..=i {
            if let Some(v) = close.get(j) {
                s += v;
                c += 1;
            }
        }
        out[i] = if c > 0 { s / c as f64 } else { 0.0 };
    }
    out
}

fn calc_macd(close: &Float64Chunked) -> Result<Vec<f64>> {
    let ema12 = calc_ema(close, 12);
    let ema26 = calc_ema(close, 26);
    let n = close.len();
    let mut macd = vec![0.0; n];
    for i in 0..n {
        macd[i] = ema12[i] - ema26[i];
    }
    let signal = calc_ema_vec(&macd, 9);
    for i in 0..n {
        macd[i] -= signal[i];
    }
    Ok(macd)
}
fn calc_ema(close: &Float64Chunked, period: usize) -> Vec<f64> {
    let vals = (0..close.len())
        .map(|i| close.get(i).unwrap_or(0.0))
        .collect::<Vec<_>>();
    calc_ema_vec(&vals, period)
}
fn calc_ema_vec(vals: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; vals.len()];
    if vals.is_empty() {
        return out;
    }
    let a = 2.0 / (period as f64 + 1.0);
    out[0] = vals[0];
    for i in 1..vals.len() {
        out[i] = a * vals[i] + (1.0 - a) * out[i - 1];
    }
    out
}
fn calc_turtle(
    close: &Float64Chunked,
    high: &Float64Chunked,
    low: &Float64Chunked,
    period: usize,
) -> Vec<i32> {
    let n = close.len();
    let mut out = vec![0; n];
    for i in period..n {
        let mut h = f64::NEG_INFINITY;
        let mut l = f64::INFINITY;
        for j in (i - period)..i {
            h = h.max(high.get(j).unwrap_or(h));
            l = l.min(low.get(j).unwrap_or(l));
        }
        let c = close.get(i).unwrap_or(0.0);
        if c > h {
            out[i] = 1;
        } else if c < l {
            out[i] = -1;
        }
    }
    out
}
fn calc_csm_signal(close: &Float64Chunked) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in 21..n {
        let now = close.get(i).unwrap_or(0.0);
        let prev = close.get(i - 21).unwrap_or(now);
        if prev > 0.0 {
            out[i] = (now / prev) - 1.0;
        }
    }
    out
}
fn calc_linear_score(
    close: &Float64Chunked,
    macd: &[f64],
    sma200: &[f64],
    train: usize,
) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in train..n {
        let c = close.get(i).unwrap_or(0.0);
        let prev21 = if i >= 21 {
            close.get(i - 21).unwrap_or(c)
        } else {
            c
        };
        let mom = if prev21 > 0.0 {
            (c / prev21) - 1.0
        } else {
            0.0
        };
        let sma_gap = if sma200[i] > 0.0 {
            c / sma200[i] - 1.0
        } else {
            0.0
        };
        out[i] = 0.6 * mom + 0.3 * macd[i] - 0.2 * sma_gap;
    }
    out
}
