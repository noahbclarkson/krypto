use krypto::algo::optimization::Optimizer;
use krypto::algo::strategies::{
    AtrBreakout, BollingerReversion, DynamicTrend, MacdTrend, RsiMeanReversion,
};
use krypto::algo::SignalGenerator;
use krypto::backtest::engine::Backtester;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let loader = DataLoader::new(None, None);
    println!("Loading BTCFDUSD 4h (cache)...");
    let df = loader.fetch_data("BTCFDUSD", "4h", 2000).await?;
    println!("Loaded {} candles", df.height());
    let df_tech = FeatureEngine::add_technicals(&df, None)?;
    let split = (df_tech.height() as f64 * 0.6) as usize;
    let train_df = df_tech.slice(0, split);
    let test_df = df_tech.slice(split as i64, df_tech.height() - split);
    println!("Train: {} | Test: {}", train_df.height(), test_df.height());

    let backtester = Backtester::new(10_000.0, 0.0, 0.001);
    let optimizer = Optimizer::new(80, 0.6);

    macro_rules! test_strat {
        ($name:expr, $s:expr) => {{
            let mut strat = $s;
            let (_, train_result) = optimizer.optimize(&mut strat, &train_df);
            match train_result {
                Some(res) => {
                    let sig = strat.predict(&test_df).unwrap_or_default();
                    let tr = backtester.run(&test_df, &sig, 0.05, 0.10).unwrap();
                    let rob = tr.sharpe_ratio / res.sharpe_ratio.abs().max(0.001);
                    println!("{:<25} train[sh={:.2} pf={:.2} t={}]  test[sh={:.2} t={} pnl={:.1}% rob={:.2}]",
                        $name, res.sharpe_ratio, res.profit_factor, res.total_trades,
                        tr.sharpe_ratio, tr.total_trades, tr.total_return_pct, rob);
                }
                None => println!("{:<25} optimizer returned None", $name),
            }
        }};
    }

    test_strat!("DynamicTrend", DynamicTrend::default());
    test_strat!("RsiMeanReversion", RsiMeanReversion::default());
    test_strat!("MacdTrend", MacdTrend::default());
    test_strat!("BollingerReversion", BollingerReversion::default());
    test_strat!("AtrBreakout", AtrBreakout::default());

    Ok(())
}
