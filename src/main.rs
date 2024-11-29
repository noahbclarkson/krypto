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
        TradingStrategy, TradingStrategyCrossover, TradingStrategyFitnessFunction,
        TradingStrategyGenomeBuilder, TradingStrategyMutation,
    },
};
use tracing::{error, info};

const MAX_N: usize = 50;
const MAX_DEPTH: usize = 40;

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

    let population_size = 250;
    let selection_ratio = 0.7;
    let num_individuals_per_parents = 2;
    let mutation_rate = 0.015;
    let reinsertion_ratio = 0.7;
    let generation_limit = 100; // Adjust as needed

    let available_tickers = config.symbols.clone();
    let available_intervals = config.intervals.clone();

    let config = Arc::new(config);
    let dataset = Arc::new(dataset);

    let initial_population: Population<TradingStrategy> = build_population()
        .with_genome_builder(TradingStrategyGenomeBuilder::new(
            available_tickers.clone(),
            available_intervals.clone(),
            MAX_N,
            MAX_DEPTH,
        ))
        .of_size(population_size)
        .uniform_at_random();

    let ga = genetic_algorithm()
        .with_evaluation(TradingStrategyFitnessFunction::new(
            config.clone(),
            dataset.clone(),
        ))
        .with_selection(MaximizeSelector::new(
            selection_ratio,
            num_individuals_per_parents,
        ))
        .with_crossover(TradingStrategyCrossover)
        .with_mutation(TradingStrategyMutation::new(
            mutation_rate,
            available_tickers.clone(),
            available_intervals.clone(),
            MAX_N,
            MAX_DEPTH,
        ))
        .with_reinsertion(ElitistReinserter::new(
            TradingStrategyFitnessFunction::new(config, dataset),
            true,
            reinsertion_ratio,
        ))
        .with_initial_population(initial_population)
        .build();

    let mut sim = simulate(ga)
        .until(GenerationLimit::new(generation_limit))
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
                info!(
                    "Generation {}: Best fitness: {:.2}%, Strategy: {:?}",
                    step.iteration, best_solution.solution.fitness as f64 / 100.0, best_solution.solution.genome
                );
                csv.write_record([
                    step.iteration.to_string(),
                    (best_solution.solution.fitness as f64 / 100.0).to_string(),
                    best_solution.solution.genome.to_string(),
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
                // Display the best trading strategy
                info!("Best trading strategy: {}", best_solution.solution.genome);
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
