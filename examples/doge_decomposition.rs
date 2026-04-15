use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const HOLD_BARS: usize = 54;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let mut cache = HashMap::new();

    let symbols = vec![
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    ];

    runtime.block_on(async {
        let loader = DataLoader::new(None, None);
        for &sym in &symbols {
            if let Ok(df) = loader.fetch_data(sym, "1d", CANDLES).await {
                cache.insert(sym.to_string(), df);
            }
        }
    });

    let sample_df = cache.values().next().unwrap();
    let n = sample_df.height();
    let mut ad_series = HashMap::new();

    let period = 47;
    let alpha1 = 2.0 / (period as f64 + 1.0);
    let alpha2 = 2.0 / (period as f64 * 2.0 + 1.0);

    for &sym in &symbols {
        let df = cache.get(sym).unwrap();
        let c_series = df.column("close")?.f64()?;
        let mut c = vec![0.0; n];
        for i in 0..n {
            c[i] = c_series.get(i).unwrap_or(0.0);
        }

        let mut ema1 = vec![0.0; n];
        let mut ema2 = vec![0.0; n];
        ema1[0] = c[0];
        ema2[0] = c[0];
        for i in 1..n {
            ema1[i] = alpha1 * c[i] + (1.0 - alpha1) * ema1[i - 1];
            ema2[i] = alpha2 * c[i] + (1.0 - alpha2) * ema2[i - 1];
        }
        let mut ad = vec![0.0; n];
        for i in 0..n {
            ad[i] = ema1[i] - ema2[i];
        }
        ad_series.insert(sym, ad);
    }

    let mut sym_equity = HashMap::new();
    let mut sym_trades = HashMap::new();
    for &sym in &symbols {
        sym_equity.insert(sym, 1.0f64);
        sym_trades.insert(sym, 0);
    }

    let top_k = 2; // Matching POSITION_CAP = 2 from ddbudget

    let mut base5_equity = 1.0f64;
    let mut nodoge_equity = 1.0f64;

    let mut i = TRAIN_BARS;
    while i < n.saturating_sub(HOLD_BARS + 1) {
        let train_start = i.saturating_sub(TRAIN_BARS);

        // Proper train loop mean calculation to avoid lookahead
        let mut ad_means = HashMap::new();
        for &sym in &symbols {
            let ad = &ad_series[sym];
            let mut sum = 0.0;
            let mut count = 0;
            for j in train_start..i {
                if ad[j] != 0.0 {
                    sum += ad[j];
                    count += 1;
                }
            }
            let mean = if count > 0 { sum / count as f64 } else { 0.0 };
            ad_means.insert(sym, mean);
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        let mut nodoge_scores: Vec<(&str, f64)> = Vec::new();

        for &sym in &symbols {
            let ad = ad_series[sym][i];
            let mean = ad_means[sym];
            if ad > mean {
                scores.push((sym, ad));
                if *sym != *"DOGEUSDT" {
                    nodoge_scores.push((sym, ad));
                }
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        nodoge_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let top: Vec<_> = scores.iter().take(top_k).collect();
        let top_nodoge: Vec<_> = nodoge_scores.iter().take(top_k).collect();

        if top.is_empty() {
            i += 1;
            continue;
        }

        let mut trade_pnl = 1.0;
        let capital_per_trade = 1.0 / top_k as f64; // 50% allocation per position

        for &(sym, _) in &top {
            let df = cache.get(*sym).unwrap();
            let o_series = df.column("open")?.f64()?;
            let entry = o_series.get(i + 1).unwrap_or(0.0);
            let exit = o_series.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let ret = (exit / entry) * (1.0 - TAKER_FEE) - 1.0;
                trade_pnl += capital_per_trade * ret;

                *sym_equity.get_mut(*sym).unwrap() *= 1.0 + ret;
                *sym_trades.get_mut(*sym).unwrap() += 1;
            }
        }
        base5_equity *= trade_pnl;

        let mut nodoge_trade_pnl = 1.0;
        for &(sym, _) in &top_nodoge {
            let df = cache.get(*sym).unwrap();
            let o_series = df.column("open")?.f64()?;
            let entry = o_series.get(i + 1).unwrap_or(0.0);
            let exit = o_series.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let ret = (exit / entry) * (1.0 - TAKER_FEE) - 1.0;
                nodoge_trade_pnl += capital_per_trade * ret;
            }
        }
        nodoge_equity *= nodoge_trade_pnl;

        i += HOLD_BARS + 1;
    }

    println!("A/D period=47 Component Attribution (Position Cap = 2, 54-Bar Hold)\n");
    println!("| Symbol  | Unlevered Comp Ret | Trades |");
    println!("|---------|--------------------|--------|");
    for &sym in &symbols {
        let comp_ret = (sym_equity[sym] - 1.0) * 100.0;
        let count = sym_trades[sym];
        println!("| {:<8}| {:>17.1}% | {:>6} |", sym, comp_ret, count);
    }

    println!("\nPortfolio Level (Non-overlapping, capital-allocated):");
    println!(
        "Base5 (with DOGE) : {:>12.1}%",
        (base5_equity - 1.0) * 100.0
    );
    println!(
        "NoDOGE            : {:>12.1}%",
        (nodoge_equity - 1.0) * 100.0
    );

    Ok(())
}
