use krypto::algo::ensemble::{MetaEnsemble, VotingEnsemble};
use krypto::algo::strategies::{BollingerReversion, DynamicTrend, RsiMeanReversion};
use krypto::algo::SignalGenerator;
use krypto::data::loader::DataLoader;
use krypto::features::frac_diff::frac_diff_ffd;
use krypto::features::indicators::FeatureEngine;
use krypto::labeling::triple_barrier::apply_triple_barrier;
use polars::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let loader = DataLoader::new(None, None);
    let symbol = "BTCUSDT";

    println!("Fetching data for {symbol}...");
    let mut df = loader.fetch_data(symbol, "1h", 1000).await?;

    println!("Computing technical indicators...");
    df = FeatureEngine::add_technicals(&df, None)?;

    println!("Applying Fractional Differentiation...");
    let close_series = df.column("close")?;
    let fd_series = frac_diff_ffd(close_series, 0.4, 20)?;

    let mut df = df.clone();
    df.with_column(fd_series)?;

    println!("Generating Triple Barrier Labels...");
    let close_f64 = close_series.f64()?;
    let mut returns_vec = Vec::with_capacity(close_f64.len());
    returns_vec.push(0.0);
    for i in 1..close_f64.len() {
        let curr = close_f64.get(i).unwrap_or(0.0);
        let prev = close_f64.get(i - 1).unwrap_or(0.0);
        if prev != 0.0 {
            returns_vec.push((curr - prev) / prev);
        } else {
            returns_vec.push(0.0);
        }
    }
    let vol_series = Series::new("volatility", returns_vec);

    let _labels = apply_triple_barrier(&df, &vol_series, 24, (2.0, 1.0))?;

    // Build ensemble with multiple orthogonal strategies
    let mut ensemble = MetaEnsemble::new();
    ensemble.add_strategy(Box::new(DynamicTrend::default()));
    ensemble.add_strategy(Box::new(RsiMeanReversion::default()));
    ensemble.add_strategy(Box::new(BollingerReversion::default()));

    println!("Running Meta-Ensemble with 3 orthogonal strategies...");
    let signal = ensemble.generate_signal(&df)?;

    // Also test VotingEnsemble for consensus-based signals
    let voting = VotingEnsemble::new(
        vec![
            Box::new(DynamicTrend::default()),
            Box::new(RsiMeanReversion::default()),
            Box::new(BollingerReversion::default()),
        ],
        2, // min_agree: need 2 of 3 to agree
    );

    println!("Running VotingEnsemble (requires 2/3 agreement)...");
    let vote_series = voting.predict(&df)?;
    let vote_signal = vote_series.f64()?.last().unwrap_or(0.0);

    println!("Meta-Signal: {signal:.4}");
    println!("Vote-Signal: {vote_signal:.4}");

    if signal > 0.5 {
        println!("ACTION: BUY");
    } else if signal < -0.5 {
        println!("ACTION: SELL");
    } else {
        println!("ACTION: HOLD");
    }

    Ok(())
}
