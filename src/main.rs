use core::f64;
use std::sync::Arc;

use genevo::{
    ga::genetic_algorithm,
    operator::prelude::{ElitistReinserter, MaximizeSelector},
    prelude::{build_population, simulate, GenerationLimit, Population},
    simulation::*,
};
use krypto::{
    config::KryptoConfig,
    data::dataset::Dataset,
    error::KryptoError,
    logging::setup_tracing,
    optimisation::{
        TradingStrategyCrossover, TradingStrategyFitnessFunction, TradingStrategyGenome,
        TradingStrategyGenomeBuilder, TradingStrategyMutation,
    },
};
use tracing::{error, info};

pub fn main() {
    let (_, file_guard) = setup_tracing(Some("logs")).expect("Failed to set up tracing");
    let result = run();
    if let Err(e) = result {
        error!("Error: {:?}", e);
    }
    drop(file_guard);
}

fn run() -> Result<(), KryptoError> {
    let config = KryptoConfig::read_config::<&str>(None)?;
    let dataset = Dataset::load(&config)?;

    let selection_ratio = 0.7;
    let num_individuals_per_parents = 2;
    let reinsertion_ratio = 0.7;

    let available_tickers = config.symbols.clone();
    let available_intervals = config.intervals.clone();
    let available_tecnicals = config.technicals.clone();

    let config = Arc::new(config);
    let dataset = Arc::new(dataset);

    let initial_population: Population<TradingStrategyGenome> = build_population()
        .with_genome_builder(TradingStrategyGenomeBuilder::new(
            available_tickers.clone(),
            available_intervals.clone(),
            available_tecnicals.clone(),
            config.max_n,
            config.max_depth,
        ))
        .of_size(config.population_size)
        .uniform_at_random();

    let ga = genetic_algorithm()
        .with_evaluation(TradingStrategyFitnessFunction::new(
            config.clone(),
            dataset.clone(),
            available_tickers.clone(),
            available_tecnicals.clone(),
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
            config.max_n,
            config.max_depth,
        ))
        .with_reinsertion(ElitistReinserter::new(
            TradingStrategyFitnessFunction::new(
                config.clone(),
                dataset,
                available_tickers.clone(),
                available_tecnicals.clone(),
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
                    .to_phenotype(&available_tickers, &available_tecnicals);
                info!(
                    "Generation {}: Best fitness: {:.2}%, Strategy: {:?}",
                    step.iteration,
                    best_solution.solution.fitness as f64 / 100.0,
                    phenotype
                );
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
                    .to_phenotype(&available_tickers, &available_tecnicals);
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
