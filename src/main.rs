use std::{
    fs::File, // Keep File import
    // path::PathBuf, // Remove unused import
    sync::Arc,
    time::Duration,
};

use binance::rest_model::OrderSide; // Keep this import
use genevo::{
    ga::genetic_algorithm,
    operator::prelude::{ElitistReinserter, MaximizeSelector},
    // Add FitnessFunction trait import
    prelude::{build_population, simulate, FitnessFunction, GenerationLimit, Population},
    simulation::*,
};
use krypto::{
    algorithm::{
        algo::{Algorithm, AlgorithmResult, AlgorithmSettings}, // Keep AlgorithmResult
        pls::predict,
    },
    config::{KryptoConfig, Mission},
    data::{dataset::overall_dataset::Dataset, interval::Interval},
    error::KryptoError,
    logging::setup_tracing,
    optimisation::{
        // Import the new setup function and the report generation function
        generate_trade_log_for_best, setup_report_dirs, TradingStrategyCrossover,
        TradingStrategyFitnessFunction, TradingStrategyGenome, TradingStrategyGenomeBuilder,
        TradingStrategyMutation,
    },
    trading::krypto_account::KryptoAccount,
};
use tokio::signal;
use tracing::{error, info, warn};

#[tokio::main]
pub async fn main() -> Result<(), KryptoError> {
    // Setup tracing first
    let (_non_blocking_writer_guard, file_guard) =
        setup_tracing(Some("logs")).expect("Failed to set up tracing");

    let config_result = KryptoConfig::read_config::<&str>(None).await;
    let config = match config_result {
        Ok(cfg) => Arc::new(cfg),
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            drop(file_guard); // Ensure guard is dropped on error
            return Err(e);
        }
    };

    info!("Starting Krypto application...");
    info!("Loaded configuration: {}", *config);

    let mission = config.mission.clone();

    // Setup graceful shutdown handler
    let shutdown_signal = signal::ctrl_c();
    tokio::select! {
        biased; // Prioritize shutdown signal
        _ = shutdown_signal => {
            info!("Received Ctrl+C, initiating graceful shutdown...");
            // Perform any necessary cleanup here (like closing open positions in trade loop)
            // The drop(file_guard) below will handle log flushing.
        }
        result = run_mission(config.clone(), mission) => {
            if let Err(e) = result {
                error!("Mission failed: {}", e);
                drop(file_guard); // Drop guard on mission failure too
                return Err(e);
            } else {
                info!("Mission completed successfully.");
            }
        }
    }

    info!("Krypto application shutting down.");
    drop(file_guard); // Ensure logs are flushed on normal exit or shutdown
    Ok(())
}

// Separated mission logic from main setup
async fn run_mission(config: Arc<KryptoConfig>, mission: Mission) -> Result<(), KryptoError> {
    match mission {
        Mission::Optimise => optimise(config).await,
        Mission::Backtest => backtest(config).await,
        Mission::Trade => trade(config).await,
    }
}

// --- Backtest Mission ---
async fn backtest(config: Arc<KryptoConfig>) -> Result<(), KryptoError> {
    info!("Starting Backtest Mission...");
    let dataset = Dataset::load(&config).await?;

    if dataset.is_empty() {
        warn!("Dataset loaded successfully but contains no interval data. Exiting backtest.");
        return Ok(());
    }

    // Determine margin range from config
    let mut margin_values = Vec::new();
    let mut current_margin = config.backtest_margin_start;
    while current_margin <= config.backtest_margin_end {
        margin_values.push(current_margin);
        current_margin += config.backtest_margin_step;
        if config.backtest_margin_step <= 0.0 {
            break;
        } // Prevent infinite loop
    }
    if !margin_values.contains(&config.backtest_margin_end) && config.backtest_margin_step > 0.0 {
        margin_values.push(config.backtest_margin_end); // Ensure end value is included
    }

    for (interval, interval_dataset) in dataset.get_map() {
        info!("--- Backtesting Interval: {} ---", interval);
        for symbol in &config.symbols {
            info!("--- Backtesting Symbol: {} ---", symbol);
            for margin in &margin_values {
                let mut current_config = (*config).clone(); // Clone inner config
                current_config.margin = *margin;

                // Use configured max_n and max_depth for backtesting a single strategy
                // TODO: Allow loading a specific strategy from GA results for backtesting?
                let settings = AlgorithmSettings::new(config.max_n, config.max_depth, symbol);

                match Algorithm::load(interval_dataset, settings.clone(), &current_config) {
                    Ok(algorithm) => {
                        info!(
                            "Margin: {:.1}x | Walk-Forward Result: {}",
                            *margin, algorithm.result // algorithm.result is the AlgorithmResult from walk-forward
                        );

                        // Optionally, run backtest on all seen data for comparison (likely optimistic)
                        match algorithm.backtest_on_all_seen_data(interval_dataset, &current_config)
                        {
                            Ok(full_result) => {
                                info!(
                                    "Margin: {:.1}x | Full Data Result:    {}", // full_result is also AlgorithmResult
                                    *margin, full_result
                                )
                            }
                            Err(e) => warn!(
                                "Margin: {:.1}x | Failed to run backtest on full data: {}",
                                *margin, e
                            ),
                        }
                    }
                    Err(e) => {
                        error!(
                            "Margin: {:.1}x | Failed to load algorithm for {} on {}: {}",
                            *margin, symbol, interval, e
                        );
                    }
                }
            }
        }
    }
    info!("Backtest Mission Finished.");
    Ok(())
}

// --- Optimise Mission ---
async fn optimise(config: Arc<KryptoConfig>) -> Result<(), KryptoError> {
    info!("Starting Optimisation Mission...");

    // --- Setup Reporting ---
    let report_path = setup_report_dirs()?; // Get the base report path
    let summary_csv_path = report_path.join("optimization_summary.csv");

    // Ensure the file is created/truncated before creating the writer
    File::create(&summary_csv_path).map_err(KryptoError::IoError)?;

    let mut summary_writer = match csv::Writer::from_path(&summary_csv_path) {
        Ok(writer) => writer,
        Err(e) => {
            error!(
                "Failed to create summary CSV writer for '{}': {}",
                summary_csv_path.display(),
                e
            );
            return Err(KryptoError::CsvError(e));
        }
    };

    // Define the FULL header row for the summary CSV
    let headers = vec![
        "Generation".to_string(),
        "BestFitnessScore".to_string(),
        "BestMonthlyReturn".to_string(),
        "BestAccuracy".to_string(),
        "BestSharpeRatio".to_string(),
        "BestMaxDrawdown".to_string(),
        "BestTotalTrades".to_string(),
        "BestWinRate".to_string(),
        // Add other best KPIs here if AlgorithmResult is expanded
        "AvgFitnessScore".to_string(),
        "AvgMonthlyReturn".to_string(),
        "AvgAccuracy".to_string(),
        "AvgSharpeRatio".to_string(),
        "AvgMaxDrawdown".to_string(),
        "AvgTotalTrades".to_string(),
        "AvgWinRate".to_string(),
        // Add other avg KPIs here if AlgorithmResult is expanded
        "BestStrategyPhenotype".to_string(),
    ];
    summary_writer.write_record(&headers)?;
    summary_writer.flush()?; // Flush header immediately

    // --- Load Data ---
    let dataset = Arc::new(Dataset::load(&config).await?);
    if dataset.is_empty() {
        warn!("Dataset loaded successfully but contains no interval data. Exiting optimisation.");
        return Ok(());
    }

    // --- GA Setup ---
    let available_tickers = config.symbols.clone();
    let available_intervals = config.intervals.clone();
    let available_technicals = config.technicals.clone();

    config.validate()?; // Validate config before using GA params

    let fitness_function = TradingStrategyFitnessFunction::new(
        config.clone(),
        dataset.clone(),
        available_tickers.clone(),
        available_technicals.clone(),
    );

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
        .with_evaluation(fitness_function.clone()) // Pass the fitness function instance
        .with_selection(MaximizeSelector::new(
            config.selection_ratio,
            config.num_individuals_per_parents,
        ))
        .with_crossover(TradingStrategyCrossover {
            available_tickers: Arc::new(available_tickers.clone()),
        })
        .with_mutation(TradingStrategyMutation::new(
            config.mutation_rate,
            Arc::new(available_tickers.clone()),
            Arc::new(available_intervals.clone()),
            config.max_depth,
            config.max_n,
        ))
        .with_reinsertion(ElitistReinserter::new(
            fitness_function.clone(), // Pass fitness function instance again
            true,                     // Keep elites
            config.reinsertion_ratio,
        ))
        .with_initial_population(initial_population)
        .build();

    let mut sim = simulate(ga)
        .until(GenerationLimit::new(config.generation_limit))
        .build();

    // --- Simulation Loop ---
    info!("Starting Genetic Algorithm Simulation...");

    // Track overall best strategy found across all generations
    // Use the trait method via the fitness_function instance
    let mut overall_best_fitness: i64 = fitness_function.lowest_possible_fitness();
    let mut overall_best_genome: Option<TradingStrategyGenome> = None;

    loop {
        let result = sim.step();

        match result {
            Ok(SimResult::Intermediate(step)) => {
                let current_gen = step.iteration;
                let best_solution_genome = &step.result.best_solution.solution.genome;

                // --- Get Best Solution Metrics ---
                // Use evaluate_genome which handles caching
                let (best_fitness_score, best_result_metrics) =
                    match fitness_function.evaluate_genome(best_solution_genome) {
                        Ok(result) => result,
                        Err(e) => {
                            error!(
                                "Gen {}: Failed to retrieve metrics for best genome: {}. Using default.",
                                current_gen, e
                            );
                            (
                                // Use trait method via instance
                                fitness_function.lowest_possible_fitness(),
                                AlgorithmResult::new(-1.0, 0.0, -999.0, 1.0, 0, 0.0), // Default/worst result
                            )
                        }
                    };

                // --- Calculate Average Metrics ---
                let mut generation_results: Vec<AlgorithmResult> = Vec::new();
                let mut generation_scores: Vec<i64> = Vec::new();

                // Iterate through the evaluated population of the *current* step
                // Fix: Use .iter() on the Vec inside the Rc
                for individual in step.result.evaluated_population.individuals().iter() {
                    // Retrieve cached results for each individual
                    match fitness_function.evaluate_genome(individual) {
                        Ok((score, metrics)) => {
                            generation_scores.push(score);
                            generation_results.push(metrics);
                        }
                        Err(e) => {
                            warn!(
                                "Gen {}: Failed to retrieve cached metrics for an individual: {}. Skipping for avg calc.",
                                current_gen, e
                            );
                        }
                    }
                }

                // Use trait method via instance
                let avg_fitness_score = fitness_function.average(&generation_scores);

                let num_results = generation_results.len() as f64;
                let avg_metrics = if num_results > 0.0 {
                    // Calculate sum for each metric, ensuring finite values are handled if necessary
                    let sum_monthly_return: f64 = generation_results.iter().map(|r| r.monthly_return).filter(|v| v.is_finite()).sum();
                    let sum_accuracy: f64 = generation_results.iter().map(|r| r.accuracy).filter(|v| v.is_finite()).sum();
                    let sum_sharpe: f64 = generation_results.iter().map(|r| r.sharpe_ratio).filter(|v| v.is_finite()).sum();
                    let sum_drawdown: f64 = generation_results.iter().map(|r| r.max_drawdown).filter(|v| v.is_finite()).sum();
                    let sum_trades: f64 = generation_results.iter().map(|r| r.total_trades as f64).sum(); // Trades are u32
                    let sum_win_rate: f64 = generation_results.iter().map(|r| r.win_rate).filter(|v| v.is_finite()).sum();
                    let count_finite = generation_results.iter().filter(|r| r.sharpe_ratio.is_finite()).count() as f64; // Example count for metrics that can be non-finite

                    AlgorithmResult::new(
                        if count_finite > 0.0 { sum_monthly_return / count_finite } else { -1.0 },
                        if count_finite > 0.0 { sum_accuracy / count_finite } else { 0.0 },
                        if count_finite > 0.0 { sum_sharpe / count_finite } else { -999.0 },
                        if count_finite > 0.0 { sum_drawdown / count_finite } else { 1.0 },
                        (sum_trades / num_results).round() as u32, // Average trades over all results
                        if count_finite > 0.0 { sum_win_rate / count_finite } else { 0.0 },
                    )
                } else {
                    AlgorithmResult::new(-1.0, 0.0, -999.0, 1.0, 0, 0.0) // Default/worst average
                };

                // --- Get Phenotype String ---
                let phenotype_str = match best_solution_genome
                    .to_phenotype(&available_tickers, &available_technicals)
                {
                    Ok(phenotype) => phenotype.to_string(),
                    Err(e) => {
                        warn!(
                            "Gen {}: Could not convert best genome to phenotype for logging: {}",
                            current_gen, e
                        );
                        format!("Error: {}", e)
                    }
                };

                // --- Log to Console ---
                info!(
                    "Generation {:>4}: Best Fitness: {:>10} (Sharpe: {:.2}) | Avg Fitness: {:>10} (Avg Sharpe: {:.2}) | Strategy: {}",
                    current_gen,
                    best_fitness_score,
                    best_result_metrics.sharpe_ratio,
                    avg_fitness_score,
                    avg_metrics.sharpe_ratio,
                    phenotype_str
                );

                // --- Write Full Row to Summary CSV ---
                let record = vec![
                    current_gen.to_string(),
                    best_fitness_score.to_string(),
                    format!("{:.6}", best_result_metrics.monthly_return),
                    format!("{:.4}", best_result_metrics.accuracy),
                    format!("{:.4}", best_result_metrics.sharpe_ratio),
                    format!("{:.4}", best_result_metrics.max_drawdown),
                    best_result_metrics.total_trades.to_string(),
                    format!("{:.4}", best_result_metrics.win_rate),
                    // Add other best KPIs if needed
                    avg_fitness_score.to_string(),
                    format!("{:.6}", avg_metrics.monthly_return),
                    format!("{:.4}", avg_metrics.accuracy),
                    format!("{:.4}", avg_metrics.sharpe_ratio),
                    format!("{:.4}", avg_metrics.max_drawdown),
                    avg_metrics.total_trades.to_string(),
                    format!("{:.4}", avg_metrics.win_rate),
                    // Add other avg KPIs if needed
                    phenotype_str,
                ];

                if let Err(e) = summary_writer.write_record(&record) {
                    error!("Gen {}: Failed to write summary record to CSV: {}", current_gen, e);
                };
                // Flush periodically
                if current_gen % 2 == 0 {
                    if let Err(e) = summary_writer.flush() {
                        error!("Gen {}: Failed to flush summary CSV: {}", current_gen, e);
                    }
                }

                // --- Check for New Overall Best and Generate Report ---
                if best_fitness_score > overall_best_fitness {
                    info!(
                        "*** New overall best fitness found! Gen: {}, Fitness: {}, Previous: {} ***",
                        current_gen, best_fitness_score, overall_best_fitness
                    );
                    overall_best_fitness = best_fitness_score;
                    overall_best_genome = Some(best_solution_genome.clone());

                    // Trigger top fitness report generation
                    if let Some(best_genome) = &overall_best_genome {
                        match generate_trade_log_for_best( // Use the function from optimisation module
                            best_genome,
                            &report_path, // Pass base report path
                            &config,
                            &dataset,
                            &available_tickers,
                            &available_technicals,
                        ) {
                            Ok(_) => info!("Successfully generated report files for new best strategy."),
                            Err(e) => error!("Failed to generate report files for new best strategy: {}", e),
                        }
                    }
                }
            }
            Ok(SimResult::Final(step, processing_time, _, stop_reason)) => {
                let final_gen = step.iteration;
                let best_solution = &step.result.best_solution;
                let best_solution_genome = &best_solution.solution.genome;

                // --- Get Final Best Solution Metrics ---
                let (final_best_fitness_score, final_best_result_metrics) =
                    match fitness_function.evaluate_genome(best_solution_genome) {
                        Ok(result) => result,
                        Err(e) => {
                            error!(
                                "Final: Failed to retrieve metrics for best genome: {}. Using default.",
                                e
                            );
                            (
                                // Use trait method via instance
                                fitness_function.lowest_possible_fitness(),
                                AlgorithmResult::new(-1.0, 0.0, -999.0, 1.0, 0, 0.0),
                            )
                        }
                    };

                // --- Calculate Final Average Metrics (Optional) ---
                let avg_fitness_score = *step.result.evaluated_population.average_fitness();

                // --- Get Phenotype String ---
                let phenotype_str = match best_solution_genome
                    .to_phenotype(&available_tickers, &available_technicals)
                {
                    Ok(phenotype) => phenotype.to_string(),
                    Err(e) => {
                        warn!(
                            "Final: Could not convert best genome to phenotype for logging: {}",
                            e
                        );
                        format!("Error: {}", e)
                    }
                };

                // --- Log Final Summary ---
                info!("--------------------------------------------------");
                info!(
                    "Optimisation Finished: Reason: {} | Total Time: {:.3}s", // Use .duration()
                    stop_reason,
                    processing_time.duration().num_seconds()
                );
                info!(
                    "Best strategy found in generation {}: Fitness: {} (Sharpe: {:.2})",
                    best_solution.generation,
                    final_best_fitness_score,
                    final_best_result_metrics.sharpe_ratio
                );
                info!("Best strategy details: {}", phenotype_str);
                info!("--------------------------------------------------");

                // --- Write Final Row to Summary CSV ---
                let final_record = vec![
                    final_gen.to_string(),
                    final_best_fitness_score.to_string(),
                    format!("{:.6}", final_best_result_metrics.monthly_return),
                    format!("{:.4}", final_best_result_metrics.accuracy),
                    format!("{:.4}", final_best_result_metrics.sharpe_ratio),
                    format!("{:.4}", final_best_result_metrics.max_drawdown),
                    final_best_result_metrics.total_trades.to_string(),
                    format!("{:.4}", final_best_result_metrics.win_rate),
                    // Add other best KPIs if needed
                    avg_fitness_score.to_string(),
                    "-".to_string(), // Placeholder for AvgMonthlyReturn
                    "-".to_string(), // Placeholder for AvgAccuracy
                    "-".to_string(), // Placeholder for AvgSharpeRatio
                    "-".to_string(), // Placeholder for AvgMaxDrawdown
                    "-".to_string(), // Placeholder for AvgTotalTrades
                    "-".to_string(), // Placeholder for AvgWinRate
                    // Add other avg KPI placeholders if needed
                    phenotype_str,
                ];
                if let Err(e) = summary_writer.write_record(&final_record) {
                    error!("Final: Failed to write summary record to CSV: {}", e);
                };
                summary_writer.flush()?; // Final flush

                // --- Final Check for Overall Best and Report Generation ---
                if final_best_fitness_score > overall_best_fitness {
                    info!("*** Final best solution is the new overall best! Fitness: {} ***", final_best_fitness_score);
                    overall_best_genome = Some(best_solution_genome.clone()); // Update overall best

                    // Trigger top fitness report generation
                    if let Some(best_genome) = &overall_best_genome {
                        match generate_trade_log_for_best(
                            best_genome,
                            &report_path,
                            &config,
                            &dataset,
                            &available_tickers,
                            &available_technicals,
                        ) {
                            Ok(_) => info!("Successfully generated report files for final best strategy."),
                            Err(e) => error!("Failed to generate report files for final best strategy: {}", e),
                        }
                    }
                } else if overall_best_genome.is_none()
                    // Use trait method via instance
                    && final_best_fitness_score > fitness_function.lowest_possible_fitness()
                {
                    // Handle case where no intermediate best was found but the final one is valid
                    info!("*** Final best solution is the only valid strategy found! Fitness: {} ***", final_best_fitness_score);
                    overall_best_genome = Some(best_solution_genome.clone());
                    // Trigger report generation
                    if let Some(best_genome) = &overall_best_genome {
                        match generate_trade_log_for_best(
                            best_genome,
                            &report_path,
                            &config,
                            &dataset,
                            &available_tickers,
                            &available_technicals,
                        ) {
                            Ok(_) => info!("Successfully generated report files for final best strategy."),
                            Err(e) => error!("Failed to generate report files for final best strategy: {}", e),
                        }
                    }
                }

                break; // Exit loop
            }
            Err(error) => {
                error!("Genetic algorithm simulation error: {}", error);
                summary_writer.flush()?; // Flush CSV even on error
                return Err(KryptoError::FitnessCalculationError(format!(
                    "GA simulation failed: {}",
                    error
                )));
            }
        }
        // Optional: Check for shutdown signal within the loop for long generations
        // tokio::select! { ... }
    }

    info!("Optimisation Mission Finished.");
    Ok(())
}

// --- Trade Mission ---
async fn trade(config: Arc<KryptoConfig>) -> Result<(), KryptoError> {
    info!("Starting Trade Mission...");

    if config.api_key.is_none() || config.api_secret.is_none() {
        error!("Cannot start Trade mission: Binance API key or secret not found in configuration.");
        return Err(KryptoError::ConfigError(
            "API key/secret missing for trading.".to_string(),
        ));
    }

    // --- Initial Setup ---
    let trade_symbol = config
        .symbols
        .first()
        .ok_or_else(|| KryptoError::ConfigError("No symbols configured for trading.".to_string()))?
        .clone();
    let trade_interval = *config.intervals.first().ok_or_else(|| {
        KryptoError::ConfigError("No intervals configured for trading.".to_string())
    })?;

    info!(
        "Trading Symbol: {}, Interval: {}",
        trade_symbol, trade_interval
    );

    let mut krypto_account = KryptoAccount::new(&config, trade_symbol.clone()).await?;
    info!(
        "Isolated margin account details fetched for {}", // Avoid logging sensitive details
        krypto_account.symbol
        // krypto_account.isolated_details().await? // Avoid logging sensitive details
    );

    // TODO: Load best GA strategy from a file instead of default settings
    // For now, use config defaults
    let settings = Arc::new(AlgorithmSettings::new(
        config.max_n,
        config.max_depth,
        &trade_symbol,
    ));
    let mut algorithm: Option<Algorithm> = None; // Keep track of the loaded algorithm

    // Function to load/update data and algorithm
    async fn update_data_and_model(
        cfg: &KryptoConfig,
        interval: Interval,
        settings: &AlgorithmSettings,
    ) -> Result<Algorithm, KryptoError> {
        // TODO: Implement incremental data loading for trading efficiency
        // Currently reloads all data, which is slow for trading.
        info!("Reloading full dataset for model update...");
        let dataset = Dataset::load(cfg).await?;
        let interval_ds = dataset
            .get(&interval)
            .ok_or_else(|| KryptoError::IntervalNotFound(interval.to_string()))?;

        // Load algorithm (trains on walk-forward, then final model on full data)
        Algorithm::load(interval_ds, settings.clone(), cfg)
    }

    // --- Trading Loop State ---
    let mut current_position: Option<(OrderSide, f64)> = None; // Store Option<(Side, EntryPrice)>

    // --- Main Trading Loop ---
    info!("Starting Trading Loop...");
    loop {
        // 1. Update Data & Model (Periodically)
        info!("Updating data and retraining model...");
        match update_data_and_model(&config, trade_interval, &settings).await {
            Ok(updated_algo) => {
                info!("Model updated. Validation Result: {}", updated_algo.result);
                algorithm = Some(updated_algo);
            }
            Err(e) => {
                error!("Failed to update data/model: {}. Skipping this cycle.", e);
                tokio::time::sleep(Duration::from_secs(60)).await; // Wait before retry
                continue;
            }
        };

        let current_algo = match algorithm {
            Some(ref algo) => algo,
            None => {
                error!("Algorithm not loaded. Cannot proceed.");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        // 2. Get Latest Prediction
        // This requires getting the *latest* features based on the *most recent* data.
        // The current `update_data_and_model` reloads everything, so we need the latest slice.
        let prediction = {
            // Inefficient: Reload dataset again just to get features
            // Needs refactoring for efficient trading feature generation
            let dataset = Dataset::load(&config).await?;
            let interval_ds = dataset
                .get(&trade_interval)
                .ok_or_else(|| KryptoError::IntervalNotFound(trade_interval.to_string()))?;
            let symbol_ds = interval_ds.get_symbol_dataset(&settings)?; // Gets features/labels/candles

            if symbol_ds.is_empty() {
                warn!("Symbol dataset is empty, cannot make prediction.");
                None
            } else {
                // Get the *last* feature vector to predict the *next* candle
                let last_features = symbol_ds.get_features().last();
                match last_features {
                    Some(features) => {
                        // Predict requires a Vec<Vec<f64>>, so wrap the last features
                        match predict(&current_algo.pls, &[features.clone()]) {
                            Ok(predictions) => predictions.first().copied(), // Get the single prediction
                            Err(e) => {
                                error!("Failed to get prediction: {}", e);
                                None
                            }
                        }
                    }
                    None => {
                        warn!("No features available in symbol dataset.");
                        None
                    }
                }
            }
        };

        let predicted_signal = match prediction {
            Some(p) => p.signum(),
            None => {
                warn!("No prediction available. Holding current position.");
                0.0 // Treat as neutral signal
            }
        };

        // 3. Determine Desired Action
        let desired_side = match predicted_signal {
            s if s > 0.0 => Some(OrderSide::Buy),
            s if s < 0.0 => Some(OrderSide::Sell),
            _ => None, // Neutral signal means close position or stay flat
        };
        info!(
            "Prediction: {:?}, Desired Side: {:?}",
            prediction, desired_side
        );

        // 4. Check Current Position & Execute Trades
        let net_position = match krypto_account.net_base_asset_position().await {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to get net position: {}. Cannot execute trades.", e);
                tokio::time::sleep(Duration::from_secs(config.trade_loop_wait_seconds)).await;
                continue;
            }
        };

        // Determine current side based on net position and precision
        let current_side = {
            // Ensure precision data is available
            let precision_data = match krypto_account.get_precision_data().await {
                 Ok(pd) => pd,
                 Err(e) => {
                     error!("Failed to get precision data: {}. Cannot determine current side.", e);
                     tokio::time::sleep(Duration::from_secs(config.trade_loop_wait_seconds)).await;
                     continue;
                 }
            };
            let step_size = precision_data.get_step_size();
            if net_position > step_size {
                Some(OrderSide::Buy)
            } else if net_position < -step_size {
                Some(OrderSide::Sell)
            } else {
                None // Considered flat
            }
        };

        info!(
            "Current Net Position: {}, Current Side: {:?}",
            net_position, current_side
        );

        // --- Trade Execution Logic ---
        let mut close_failed = false; // Track if closing the position failed

        if desired_side.is_some() && current_side != desired_side {
            // --- Signal Flip or Entry ---
            if let Some(side_to_close) = current_side {
                // Close existing position first
                info!(
                    "Signal flip detected. Closing current {:?} position.",
                    side_to_close
                );
                match krypto_account
                    .make_trade(side_to_close, true, None, &config) // reduce_only = true
                    .await
                {
                    Ok(result) => {
                        info!("Closed position successfully: {:?}", result);
                        current_position = None; // Update state
                    }
                    Err(e) => {
                        error!("Failed to close position: {}. Holding position.", e);
                        close_failed = true; // Mark close as failed
                    }
                }
            }

            // Open new position (only if close succeeded or wasn't needed)
            if !close_failed {
                if let Some(side_to_open) = desired_side {
                    info!("Entering new {:?} position.", side_to_open);
                    // Clone side_to_open before passing to make_trade
                    match krypto_account
                        .make_trade(side_to_open.clone(), false, None, &config) // reduce_only = false
                        .await
                    {
                        Ok(result) => {
                            info!("Opened position successfully: {:?}", result);
                            let entry_price = result.price; // Get entry price from result
                            // Use the original (uncloned) side_to_open for state
                            current_position = Some((side_to_open, entry_price)); // Update state
                        }
                        Err(e) => {
                            error!("Failed to open new position: {}", e);
                            current_position = None; // Ensure state reflects failure
                        }
                    }
                }
            }
        } else if desired_side.is_none() && current_side.is_some() {
            // --- Neutral Signal: Close Position ---
            if let Some(side_to_close) = current_side {
                info!(
                    "Neutral signal received. Closing current {:?} position.",
                    side_to_close
                );
                match krypto_account
                    .make_trade(side_to_close, true, None, &config) // reduce_only = true
                    .await
                {
                    Ok(result) => {
                        info!("Closed position successfully on neutral signal: {:?}", result);
                        current_position = None; // Update state
                    }
                    Err(e) => {
                        error!(
                            "Failed to close position on neutral signal: {}. Holding position.",
                            e
                        );
                        // Keep current_position state as is, since close failed
                    }
                }
            }
        } else {
            // --- Hold or Stay Flat ---
            info!(
                "Holding current position ({:?}) or staying flat.",
                current_side
            );
        }

        // 5. Check Stop-Loss / Take-Profit (if in a position)
        let mut sl_tp_triggered = false; // Flag to track if position was closed by SL/TP
        // Use `ref side` to borrow OrderSide from the tuple
        if let Some((ref side, entry_price)) = current_position {
            let market_price = match krypto_account.market.get_price(&trade_symbol).await {
                Ok(ticker_price) => ticker_price.price,
                Err(e) => {
                    warn!("Failed to get current market price for SL/TP check: {}", e);
                    -1.0 // Indicate failure
                }
            };

            if market_price > 0.0 {
                // Calculate return based on the side of the current position
                let current_return = match side {
                    OrderSide::Buy => (market_price - entry_price) / entry_price,
                    OrderSide::Sell => (entry_price - market_price) / entry_price,
                };

                // Check Stop Loss
                if let Some(sl_pct) = config.trade_stop_loss_percentage {
                    if current_return <= -sl_pct {
                        warn!(
                            "STOP LOSS triggered! Side: {:?}, Entry: {}, Current: {}, Return: {:.2}% <= -{:.2}%",
                            side, entry_price, market_price, current_return * 100.0, sl_pct * 100.0
                        );
                        // Clone side before passing as it's borrowed in the pattern
                        match krypto_account
                            .make_trade(side.clone(), true, None, &config) // reduce_only = true
                            .await
                        {
                            Ok(result) => {
                                info!("Closed position successfully via SL: {:?}", result);
                                sl_tp_triggered = true;
                            }
                            Err(e) => error!("Failed to close position via SL: {}", e),
                        }
                    }
                }

                // Check Take Profit (only if SL didn't trigger)
                if !sl_tp_triggered {
                    if let Some(tp_pct) = config.trade_take_profit_percentage {
                        if current_return >= tp_pct {
                            info!(
                                "TAKE PROFIT triggered! Side: {:?}, Entry: {}, Current: {}, Return: {:.2}% >= {:.2}%",
                                side, entry_price, market_price, current_return * 100.0, tp_pct * 100.0
                            );
                            // Clone side before passing
                            match krypto_account
                                .make_trade(side.clone(), true, None, &config) // reduce_only = true
                                .await
                            {
                                Ok(result) => {
                                    info!("Closed position successfully via TP: {:?}", result);
                                    sl_tp_triggered = true;
                                }
                                Err(e) => error!("Failed to close position via TP: {}", e),
                            }
                        }
                    }
                }
            }
        }
        // Update position state *after* SL/TP checks if triggered
        if sl_tp_triggered {
            current_position = None;
        }

        // 6. Wait for the next cycle
        let wait_duration = Duration::from_secs(config.trade_loop_wait_seconds);
        info!("Waiting for {:?} before next cycle...", wait_duration);
        tokio::select! {
            biased;
             _ = tokio::signal::ctrl_c() => {
                 info!("Ctrl+C received during trade loop, exiting...");
                 // Attempt to close any open position before exiting
                 // Use `ref side` again here
                 if let Some((ref side, _)) = current_position {
                     warn!("Attempting to close open {:?} position before shutdown...", side);
                      // Clone side before passing
                      match krypto_account.make_trade(side.clone(), true, None, &config).await {
                         Ok(res) => info!("Closed position on shutdown: {:?}", res),
                         Err(e) => error!("Failed to close position on shutdown: {}", e),
                     }
                 }
                 break; // Exit the trade loop
             }
             _ = tokio::time::sleep(wait_duration) => {
                 // Continue to next iteration
             }
        }
    } // End of loop

    info!("Trade Mission Finished.");
    Ok(())
}