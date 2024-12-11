use core::f64;
use std::sync::Arc;

use binance::rest_model::{MarginOrderResult, OrderSide};
use chrono::Utc;
use genevo::{
    ga::genetic_algorithm,
    operator::prelude::{ElitistReinserter, MaximizeSelector},
    prelude::{build_population, simulate, GenerationLimit, Population},
    simulation::*,
};
use krypto::{
    algorithm::{
        algo::{Algorithm, AlgorithmSettings},
        pls::predict,
    },
    config::{KryptoConfig, Mission},
    data::dataset::overall_dataset::Dataset,
    error::KryptoError,
    logging::setup_tracing,
    optimisation::{
        TradingStrategyCrossover, TradingStrategyFitnessFunction, TradingStrategyGenome,
        TradingStrategyGenomeBuilder, TradingStrategyMutation,
    },
    trading::krypto_account::KryptoAccount,
};
use tokio::signal;
use tracing::{error, info};

#[tokio::main]
pub async fn main() -> Result<(), KryptoError> {
    let (_, file_guard) = setup_tracing(Some("logs")).expect("Failed to set up tracing");
    let ctrl_c = signal::ctrl_c();

    tokio::select! {
        result = run() => {
            result?;
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down");
        }
    }
    drop(file_guard);
    Ok(())
}

async fn run() -> Result<(), KryptoError> {
    let config = KryptoConfig::read_config::<&str>(None).await?;
    match config.mission {
        Mission::Optimise => optimise(config).await,
        Mission::Backtest => backtest(&config).await,
        Mission::Trade => trade(&config).await,
    }
}

async fn backtest(config: &KryptoConfig) -> Result<(), KryptoError> {
    let dataset = Dataset::load(config).await?;
    for (interval, interval_dataset) in dataset.get_map() {
        for margin in 1..100 {
            let margin = margin as f64 / 10.0;
            let mut config = config.clone();
            config.margin = margin;
            let symbol = &config.symbols[0];
            let settings = AlgorithmSettings::new(config.max_n, config.max_depth, symbol);
            let algorithm = Algorithm::load(interval_dataset, settings, &config)?;
            let result = algorithm.backtest_on_all_seen_data(interval_dataset, &config);
            match result {
                Ok(result) => {
                    info!(
                        "Backtest result for {} on {}: Monthly return: {:.2}%, Accuracy: {:.2}% at margin: {:.1}",
                        symbol,
                        interval,
                        result.monthly_return * 100.0,
                        result.accuracy * 100.0,
                        margin
                    );
                }
                Err(e) => {
                    error!("Error during backtest: {}", e);
                }
            }
        }
    }
    Ok(())
}

async fn optimise(config: KryptoConfig) -> Result<(), KryptoError> {
    let dataset = Dataset::load(&config).await?;

    let selection_ratio = 0.7;
    let num_individuals_per_parents = 2;
    let reinsertion_ratio = 0.7;

    let available_tickers = config.symbols.clone();
    let available_intervals = config.intervals.clone();
    let available_technicals = config.technicals.clone();

    let config = Arc::new(config);
    let dataset = Arc::new(dataset);

    let initial_population: Population<TradingStrategyGenome> = build_population()
        .with_genome_builder(TradingStrategyGenomeBuilder::new(
            available_tickers.clone(),
            available_intervals.clone(),
            available_technicals.clone(),
            config.max_depth,
            config.max_n,
        ))
        .of_size(config.population_size)
        .uniform_at_random();

    let ga = genetic_algorithm()
        .with_evaluation(TradingStrategyFitnessFunction::new(
            config.clone(),
            dataset.clone(),
            available_tickers.clone(),
            available_technicals.clone(),
        ))
        .with_selection(MaximizeSelector::new(
            selection_ratio,
            num_individuals_per_parents,
        ))
        .with_crossover(TradingStrategyCrossover {
            available_tickers: available_tickers.clone(),
        })
        .with_mutation(TradingStrategyMutation::new(
            config.mutation_rate,
            available_tickers.clone(),
            available_intervals.clone(),
            config.max_depth,
            config.max_n,
        ))
        .with_reinsertion(ElitistReinserter::new(
            TradingStrategyFitnessFunction::new(
                config.clone(),
                dataset,
                available_tickers.clone(),
                available_technicals.clone(),
            ),
            true,
            reinsertion_ratio,
        ))
        .with_initial_population(initial_population)
        .build();

    let mut sim = simulate(ga)
        .until(GenerationLimit::new(config.generation_limit))
        .build();

    info!("Starting Genetic Algorithm");

    let mut csv = csv::Writer::from_path("ga-results.csv")?;
    csv.write_record(["Generation", "Fitness", "Strategy"])?;
    csv.flush()?;
    // Run the simulation loop
    loop {
        match sim.step() {
            Ok(SimResult::Intermediate(step)) => {
                let best_solution = &step.result.best_solution;
                let phenotype = best_solution
                    .solution
                    .genome
                    .to_phenotype(&available_tickers, &available_technicals);
                info!(
                    "Generation {}: Best fitness: {:.2}%, Strategy: {:?}",
                    step.iteration,
                    best_solution.solution.fitness as f64 / 100.0,
                    phenotype
                );
                let average = *step.result.evaluated_population.average_fitness();
                info!("Average fitness: {:.2}%", average as f64 / 100.0);
                csv.write_record([
                    step.iteration.to_string(),
                    (best_solution.solution.fitness as f64 / 100.0).to_string(),
                    phenotype.to_string(),
                ])?;
                csv.flush()?;
            }
            Ok(SimResult::Final(step, processing_time, _, stop_reason)) => {
                let best_solution = &step.result.best_solution;
                info!(
                    "Simulation ended: {} in {}s",
                    stop_reason,
                    processing_time.duration().num_seconds()
                );
                info!(
                    "Best strategy found in generation {}: Fitness: {:.2}%",
                    best_solution.generation,
                    best_solution.solution.fitness as f64 / 100.0
                );
                let phenotype = best_solution
                    .solution
                    .genome
                    .to_phenotype(&available_tickers, &available_technicals);
                // Display the best trading strategy
                info!("Best trading strategy: {}", phenotype);
                break;
            }
            Err(error) => {
                error!("Error: {}", error);
                break;
            }
        }
    }

    Ok(())
}

async fn trade(config: &KryptoConfig) -> Result<(), KryptoError> {
    let dataset = Dataset::load(config).await?;
    let symbol = &config.symbols[0];
    let settings = AlgorithmSettings::new(config.max_n, config.max_depth, symbol);
    let interval = &config.intervals[0];
    let dataset = dataset
        .get(interval)
        .ok_or(KryptoError::IntervalNotFound(interval.to_string()))?;
    let algorithm = Algorithm::load(dataset, settings.clone(), config)?;
    let result = algorithm.backtest_on_all_seen_data(dataset, config);
    match result {
        Ok(result) => {
            info!(
                "Backtest result for {} on {}: Monthly return: {:.2}%, Accuracy: {:.2}%",
                symbol,
                config.intervals[0],
                result.monthly_return * 100.0,
                result.accuracy * 100.0
            );
        }
        Err(e) => {
            error!("Error during backtest: {}", e);
        }
    }
    let pls = algorithm.pls;
    let mut position = None;
    let mut ka = KryptoAccount::new(config.clone(), symbol.clone()).await;
    let isolated_details = ka.isolated_details().await?;
    let isolated_details = &isolated_details.assets[0];
    info!("Isolated margin account details: {:?}", isolated_details);
    loop {
        let dataset = Dataset::load(config).await?;
        let interval_ds = dataset
            .get(interval)
            .ok_or(KryptoError::IntervalNotFound(interval.to_string()))?;
        let ds = interval_ds.get_symbol_dataset(&settings);
        let last_candle = ds.get_candles().last().unwrap();
        // Get time until last candle closes - 3 minutes
        let time_until_close =
            last_candle.close_time.timestamp_millis() - Utc::now().timestamp_millis() - 180000;
        if time_until_close > 0 {
            info!(
                "Waiting for {}s until next candle which closes at {}",
                time_until_close as f64 / 1000.0,
                last_candle.close_time
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(time_until_close as u64)).await;
        }
        let dataset = Dataset::load(config).await?;
        let interval_ds = dataset
            .get(interval)
            .ok_or(KryptoError::IntervalNotFound(interval.to_string()))?;
        let ds = interval_ds.get_symbol_dataset(&settings);
        let predictions = predict(&pls, ds.get_features())?;
        let last_prediction = predictions.last().unwrap();
        let side = match last_prediction.signum() {
            x if x > 0.0 => OrderSide::Buy,
            x if x < 0.0 => OrderSide::Sell,
            _ => OrderSide::Buy,
        };
        info!("Predicted side: {:?}", side);
        let pos = position.clone();
        if pos.is_some() && pos.unwrap() != side {
            let result = ka.make_trade(&side, true, None).await;
            print_trade_result(result);
            let result = ka.make_trade(&side, false, Some(0.4)).await;
            print_trade_result(result);
        } else if position.is_none() {
            let result = ka.make_trade(&side, false, Some(0.4)).await;
            print_trade_result(result);
            position = Some(side);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
    }
}

fn print_trade_result(result: Result<MarginOrderResult, KryptoError>) {
    match result {
        Ok(result) => {
            info!("Entered trade at price: {}, cumulative quote quantity: {}, executed quantity: {}, original quantity: {}, order-side: {:?}",
                  result.price, result.cummulative_quote_qty, result.executed_qty, result.orig_qty, result.side);
        }
        Err(e) => {
            error!("Failed to make trade: {}", e);
        }
    }
}
