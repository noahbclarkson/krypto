use std::{
    collections::HashMap,
    fmt,
    fs,                    // Use std::fs
    path::{Path, PathBuf}, // Use std::path
    sync::{Arc, Mutex},
};

use genevo::{
    genetic::{Children, Parents},
    operator::{CrossoverOp, MutationOp},
    prelude::{FitnessFunction, GenomeBuilder, Genotype}, // Keep these imports
    random::{Rng, SliceRandom as _},
};
use tracing::{debug, error, info, warn};

use crate::{
    algorithm::{
        algo::{Algorithm, AlgorithmResult, AlgorithmSettings}, // Import AlgorithmResult
        pls::{get_pls, predict},                               // Add pls imports
        test_data::{SimulationOutput, TestData},               // Import necessary items from test_data
    },
    config::KryptoConfig,
    data::{dataset::overall_dataset::Dataset, interval::Interval},
    error::KryptoError,
};
use binance::rest_model::OrderSide; // Import OrderSide

// --- Reporting Constants ---
const REPORT_DIR: &str = "report";
const TOP_FITNESS_DIR: &str = "top";
const OPTIMIZATION_SUMMARY_FILE: &str = "optimization_summary.csv"; // Keep consistent name

// --- Reporting Setup ---
/// Creates the base report directory and clears/creates the top fitness subdirectory.
/// Also removes the old summary file if it exists.
pub fn setup_report_dirs() -> Result<PathBuf, KryptoError> {
    let report_path = PathBuf::from(REPORT_DIR);
    let top_fitness_path = report_path.join(TOP_FITNESS_DIR);

    // Create base report directory
    fs::create_dir_all(&report_path).map_err(|e| {
        KryptoError::IoError(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to create report directory '{}': {}",
                report_path.display(),
                e
            ),
        ))
    })?;

    // Clear and recreate top fitness directory (ensures clean state at start of optimisation)
    if top_fitness_path.exists() {
        fs::remove_dir_all(&top_fitness_path).map_err(|e| {
            KryptoError::IoError(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to remove top fitness directory '{}': {}",
                    top_fitness_path.display(),
                    e
                ),
            ))
        })?;
    }
    fs::create_dir_all(&top_fitness_path).map_err(|e| {
        KryptoError::IoError(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to create top fitness directory '{}': {}",
                top_fitness_path.display(),
                e
            ),
        ))
    })?;

    // Overwrite optimization summary file at the start
    let summary_path = report_path.join(OPTIMIZATION_SUMMARY_FILE);
    if summary_path.exists() {
        fs::remove_file(&summary_path).map_err(|e| {
            KryptoError::IoError(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to remove old summary file '{}': {}",
                    summary_path.display(),
                    e
                ),
            ))
        })?;
    }
    info!(
        "Report directories setup. Base: '{}', Top: '{}'",
        report_path.display(),
        top_fitness_path.display()
    );
    Ok(report_path) // Return base report path
}

// --- Genotype ---
#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Eq)]
pub struct TradingStrategyGenome {
    n: usize,
    d: usize,
    interval: Interval,
    tickers: Vec<bool>,
    symbol: String,
    technicals: Vec<bool>,
}

impl Genotype for TradingStrategyGenome {
    type Dna = Self;
}

// --- Phenotype ---
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct TradingStrategy {
    n: usize,
    d: usize,
    interval: Interval,
    tickers: Vec<String>,
    symbol: String,
    technicals: Vec<String>,
}

impl TradingStrategyGenome {
    pub fn to_phenotype(
        &self,
        available_tickers: &[String],
        available_technicals: &[String],
    ) -> Result<TradingStrategy, KryptoError> {
        if self.tickers.len() != available_tickers.len() {
            return Err(KryptoError::ConfigError(format!(
                "Genome tickers length ({}) does not match available tickers length ({})",
                self.tickers.len(),
                available_tickers.len()
            )));
        }
        if self.technicals.len() != available_technicals.len() {
            return Err(KryptoError::ConfigError(format!(
                "Genome technicals length ({}) does not match available technicals length ({})",
                self.technicals.len(),
                available_technicals.len()
            )));
        }

        let tickers: Vec<String> = self
            .tickers
            .iter()
            .zip(available_tickers.iter())
            .filter_map(|(&use_ticker, ticker_name)| {
                if use_ticker {
                    Some(ticker_name.clone())
                } else {
                    None
                }
            })
            .collect();

        let technicals: Vec<String> = self
            .technicals
            .iter()
            .zip(available_technicals.iter())
            .filter_map(|(&use_tech, tech_name)| {
                if use_tech {
                    Some(tech_name.clone())
                } else {
                    None
                }
            })
            .collect();

        // Validation: Ensure target symbol is included and lists are not empty
        if tickers.is_empty() {
            return Err(KryptoError::FitnessCalculationError(
                "Genome resulted in empty tickers list.".to_string(),
            ));
        }
        if !tickers.contains(&self.symbol) {
            // This can happen if mutation changes the symbol but not the ticker mask
            warn!(
                "Target symbol '{}' is not included in the selected feature tickers: {:?}. This might lead to errors.",
                self.symbol, tickers
            );
            // Depending on strictness, either return error or proceed with warning
            // return Err(KryptoError::FitnessCalculationError(format!(
            //     "Target symbol '{}' missing from selected feature tickers in genome.",
            //     self.symbol
            // )));
        }
        if technicals.is_empty() {
            return Err(KryptoError::FitnessCalculationError(
                "Genome resulted in empty technicals list.".to_string(),
            ));
        }

        Ok(TradingStrategy::new(
            self.n,
            self.d,
            self.interval,
            tickers,
            self.symbol.clone(),
            technicals,
        ))
    }
}

impl TradingStrategy {
    pub fn new(
        n: usize,
        d: usize,
        interval: Interval,
        tickers: Vec<String>,
        symbol: String,
        technicals: Vec<String>,
    ) -> Self {
        Self {
            n,
            d,
            interval,
            tickers,
            symbol,
            technicals,
        }
    }
}

impl fmt::Display for TradingStrategy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Strategy(Symbol: {}, Interval: {}, n: {}, d: {}, Features: [{}], Techs: [{}])",
            self.symbol,
            self.interval,
            self.n,
            self.d,
            self.tickers.join(","),
            self.technicals.join(",")
        )
    }
}

impl From<&TradingStrategy> for AlgorithmSettings {
    fn from(strategy: &TradingStrategy) -> Self {
        Self::new(strategy.n, strategy.d, &strategy.symbol)
    }
}

// --- Genome Builder ---
pub struct TradingStrategyGenomeBuilder {
    available_tickers: Vec<String>,
    available_intervals: Vec<Interval>,
    available_technicals: Vec<String>,
    max_depth: usize,
    max_n: usize,
}

impl TradingStrategyGenomeBuilder {
    pub fn new(
        available_tickers: Vec<String>,
        available_intervals: Vec<Interval>,
        available_technicals: Vec<String>,
        max_depth: usize,
        max_n: usize,
    ) -> Self {
        assert!(
            !available_tickers.is_empty(),
            "Available tickers cannot be empty"
        );
        assert!(
            !available_intervals.is_empty(),
            "Available intervals cannot be empty"
        );
        assert!(
            !available_technicals.is_empty(),
            "Available technicals cannot be empty"
        );
        assert!(max_depth > 0, "Max depth must be > 0");
        assert!(max_n > 0, "Max n must be > 0");
        Self {
            available_tickers,
            available_intervals,
            available_technicals,
            max_depth,
            max_n,
        }
    }

    fn generate_tickers<R: Rng + ?Sized>(&self, rng: &mut R) -> (Vec<bool>, String) {
        let num_tickers = self.available_tickers.len();
        let mut tickers_mask = vec![false; num_tickers];
        // Ensure at least one ticker (the target symbol) is selected
        let target_symbol_index = rng.gen_range(0..num_tickers);
        let target_symbol = self.available_tickers[target_symbol_index].clone();
        tickers_mask[target_symbol_index] = true;
        // Randomly select additional tickers
        let num_additional_to_select = rng.gen_range(0..num_tickers); // Can select 0 additional
        for _ in 0..num_additional_to_select {
            let idx = rng.gen_range(0..num_tickers);
            tickers_mask[idx] = true; // Set to true, duplicates are fine
        }
        (tickers_mask, target_symbol)
    }

    fn generate_technicals<R: Rng + ?Sized>(&self, rng: &mut R) -> Vec<bool> {
        let num_technicals = self.available_technicals.len();
        let mut technicals_mask = vec![false; num_technicals];
        let mut num_selected = 0;
        // Try selecting randomly first
        for mask_value in technicals_mask.iter_mut() {
            if rng.gen_bool(0.5) { // 50% chance to include each technical
                *mask_value = true;
                num_selected += 1;
            }
        }
        // Ensure at least one technical is selected
        if num_selected == 0 && num_technicals > 0 {
            let idx = rng.gen_range(0..num_technicals);
            technicals_mask[idx] = true;
        }
        technicals_mask
    }
}

impl GenomeBuilder<TradingStrategyGenome> for TradingStrategyGenomeBuilder {
    fn build_genome<R>(&self, _population_index: usize, rng: &mut R) -> TradingStrategyGenome
    where
        R: Rng + Sized,
    {
        let (tickers_mask, target_symbol) = self.generate_tickers(rng);
        let technicals_mask = self.generate_technicals(rng);
        let depth = rng.gen_range(1..=self.max_depth);
        let interval = *self
            .available_intervals
            .choose(rng)
            .expect("Intervals non-empty");

        // Calculate max possible n based on selected features
        let num_selected_tickers = tickers_mask.iter().filter(|&&b| b).count();
        let num_selected_technicals = technicals_mask.iter().filter(|&&b| b).count();
        // Feature dimension = depth * num_symbols * num_technicals_per_symbol
        let feature_dimension = depth * num_selected_tickers * num_selected_technicals;
        // Max components 'n' cannot exceed feature dimension (or sample size, handled later)
        let max_possible_n = feature_dimension.min(self.max_n); // Also cap by config max_n

        // Generate n, ensuring it's at least 1 and not more than possible
        let n = if max_possible_n > 0 {
            rng.gen_range(1..=max_possible_n)
        } else {
            1 // If feature dimension is 0 for some reason, default n to 1
        };

        TradingStrategyGenome {
            n,
            d: depth,
            interval,
            tickers: tickers_mask,
            symbol: target_symbol,
            technicals: technicals_mask,
        }
    }
}

// --- Fitness Function ---
#[derive(Clone)]
pub struct TradingStrategyFitnessFunction {
    config: Arc<KryptoConfig>,
    dataset: Arc<Dataset>,
    available_tickers: Arc<Vec<String>>,
    available_technicals: Arc<Vec<String>>,
    // Cache fitness score AND the full result
    fitness_cache: Arc<Mutex<HashMap<TradingStrategyGenome, (i64, AlgorithmResult)>>>,
}

impl fmt::Debug for TradingStrategyFitnessFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cache_size = self.fitness_cache.lock().map_or(0, |guard| guard.len());
        f.debug_struct("TradingStrategyFitnessFunction")
            .field("config", &"Arc<KryptoConfig>")
            .field("dataset", &"Arc<Dataset>")
            .field("available_tickers", &self.available_tickers)
            .field("available_technicals", &self.available_technicals)
            .field(
                "fitness_cache",
                &format!("Arc<Mutex<HashMap<_, _>>> ({} entries)", cache_size),
            )
            .finish()
    }
}

impl TradingStrategyFitnessFunction {
    pub fn new(
        config: Arc<KryptoConfig>,
        dataset: Arc<Dataset>,
        available_tickers: Vec<String>,
        available_technicals: Vec<String>,
    ) -> Self {
        Self {
            config,
            dataset,
            available_tickers: Arc::new(available_tickers),
            available_technicals: Arc::new(available_technicals),
            fitness_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Helper to evaluate a single genome, handling errors and caching
    // Returns Result<(fitness_score, full_metrics)>
    pub fn evaluate_genome(
        &self,
        genome: &TradingStrategyGenome,
    ) -> Result<(i64, AlgorithmResult), KryptoError> {
        // 1. Check cache
        // Use a block to ensure the lock is released after checking/inserting
        {
            let cache = self.fitness_cache.lock().unwrap();
            if let Some(cached_result) = cache.get(genome) {
                debug!("Fitness cache hit for genome.");
                return Ok(cached_result.clone()); // Clone the cached tuple
            }
        } // Lock released here

        // 2. Convert genome to phenotype (strategy)
        let strategy = genome.to_phenotype(&self.available_tickers, &self.available_technicals)?;
        debug!("Evaluating fitness of strategy: {}", strategy);

        // 3. Get the base IntervalData for the strategy's interval
        let base_interval_data = self
            .dataset
            .get(&strategy.interval)
            .ok_or_else(|| KryptoError::IntervalNotFound(strategy.interval.to_string()))?;

        // 4. Filter IntervalData (recomputes technicals and normalization)
        // This step can be costly. Consider caching this result if phenotypes repeat often.
        let specific_interval_data = base_interval_data
            .get_specific_tickers_and_technicals(&strategy.tickers, &strategy.technicals)?;

        // 5. Get AlgorithmSettings
        let settings = AlgorithmSettings::from(&strategy);

        // 6. Load the algorithm (performs walk-forward validation)
        // This is typically the most expensive part.
        let algorithm = Algorithm::load(&specific_interval_data, settings, &self.config)?;
        let result_metrics = algorithm.result.clone(); // Clone the full result from walk-forward

        // 7. Extract fitness metric (e.g., Sharpe or Monthly Return)
        // --- CHOOSE YOUR PRIMARY FITNESS METRIC HERE ---
        let primary_fitness_metric = result_metrics.sharpe_ratio; // Example: Use Sharpe Ratio
                                                                   // let primary_fitness_metric = result_metrics.monthly_return; // Alternative: Monthly Return

        // --- Fitness Score Calculation ---
        // Scale and handle non-finite values. Ensure it aligns with lowest/highest_possible_fitness.
        let fitness_score = if primary_fitness_metric.is_finite() {
            // Example scaling: Sharpe can be negative. Add offset if needed or handle range.
            // Let's assume Sharpe * 10000 is reasonable for i64 range.
            let scaled_fitness = primary_fitness_metric * 10000.0;
            // Clamp the scaled fitness to the defined min/max bounds
            scaled_fitness.clamp(
                self.lowest_possible_fitness() as f64,
                self.highest_possible_fitness() as f64,
            ) as i64
        } else {
            warn!(
                "Non-finite fitness metric ({}) for strategy: {}. Assigning lowest fitness.",
                primary_fitness_metric, strategy
            );
            self.lowest_possible_fitness() // Assign lowest fitness if calculation failed
        };

        debug!(
            "Evaluated fitness for {}: Score {}, Result: {}",
            strategy, fitness_score, result_metrics
        );

        // 8. Store result (score and full metrics) in cache
        let result_tuple = (fitness_score, result_metrics.clone());
        // Re-acquire lock to insert
        {
            let mut cache = self.fitness_cache.lock().unwrap();
            cache.insert(genome.clone(), result_tuple.clone());
        } // Lock released

        Ok(result_tuple)
    }
}

impl FitnessFunction<TradingStrategyGenome, i64> for TradingStrategyFitnessFunction {
    fn fitness_of(&self, genome: &TradingStrategyGenome) -> i64 {
        match self.evaluate_genome(genome) {
            Ok((fitness_score, _result_metrics)) => fitness_score, // Return only the score
            Err(e) => {
                // Log the specific genome that failed if possible
                error!(
                    "Error calculating fitness for genome {:?}: {}. Assigning lowest fitness.",
                    genome, e // Log genome details
                );
                self.lowest_possible_fitness()
            }
        }
    }

    fn average(&self, fitness_values: &[i64]) -> i64 {
        if fitness_values.is_empty() {
            return 0; // Or lowest_possible_fitness? 0 seems reasonable for average of none.
        }
        // Filter out lowest possible fitness values if they represent errors?
        // let valid_fitness: Vec<_> = fitness_values
        //     .iter()
        //     .filter(|&&f| f > self.lowest_possible_fitness())
        //     .collect();
        // if valid_fitness.is_empty() {
        //     return self.lowest_possible_fitness();
        // }
        // valid_fitness.iter().map(|&&f| f as i128).sum::<i128>() as i64 / valid_fitness.len() as i64

        // Simpler average including potential error values:
        let sum = fitness_values.iter().map(|&f| f as i128).sum::<i128>();
        (sum / fitness_values.len() as i128) as i64
    }

    // Define bounds for fitness score to prevent issues with scaling/selection
    fn highest_possible_fitness(&self) -> i64 {
        i64::MAX / 2 // Use a large portion of the range but avoid exact MAX
    }
    fn lowest_possible_fitness(&self) -> i64 {
        i64::MIN / 2 // Use a large portion of the range but avoid exact MIN
    }
}

// --- Crossover Operator ---
#[derive(Clone, Debug)]
pub struct TradingStrategyCrossover {
    pub available_tickers: Arc<Vec<String>>, // Needed for repair logic
}

impl genevo::operator::GeneticOperator for TradingStrategyCrossover {
    fn name() -> String {
        "TradingStrategyCrossover".to_string()
    }
}

impl CrossoverOp<TradingStrategyGenome> for TradingStrategyCrossover {
    fn crossover<R>(
        &self,
        parents: Parents<TradingStrategyGenome>,
        rng: &mut R,
    ) -> Children<TradingStrategyGenome>
    where
        R: Rng + Sized,
    {
        assert_eq!(
            parents.len(),
            2,
            "TradingStrategyCrossover requires exactly 2 parents"
        );
        let parent1 = &parents[0];
        let parent2 = &parents[1];

        // --- Crossover Genes ---
        let child_interval = if rng.gen_bool(0.5) {
            parent1.interval
        } else {
            parent2.interval
        };
        let child_symbol = if rng.gen_bool(0.5) {
            parent1.symbol.clone()
        } else {
            parent2.symbol.clone()
        };
        let child_d = ((parent1.d + parent2.d) / 2).max(1); // Average depth, min 1

        // Crossover boolean vectors (tickers, technicals) - Uniform Crossover
        // Apply clippy suggestion: use iterators
        let mut child_tickers = parent1.tickers.clone();
        child_tickers
            .iter_mut()
            .zip(parent2.tickers.iter())
            .for_each(|(c_bit, &p2_bit)| {
                if rng.gen_bool(0.5) {
                    *c_bit = p2_bit;
                }
            });

        let mut child_technicals = parent1.technicals.clone();
        child_technicals
            .iter_mut()
            .zip(parent2.technicals.iter())
            .for_each(|(c_bit, &p2_bit)| {
                if rng.gen_bool(0.5) {
                    *c_bit = p2_bit;
                }
            });


        // --- Repair/Validation ---
        // Ensure at least one ticker is selected
        let length = child_tickers.len();
        if !child_tickers.iter().any(|&b| b) && length > 0 {
            child_tickers[rng.gen_range(0..length)] = true;
        }
        // Ensure target symbol is selected
        if let Some(target_idx) = self
            .available_tickers
            .iter()
            .position(|s| s == &child_symbol)
        {
            if target_idx < child_tickers.len() {
                child_tickers[target_idx] = true; // Force selection
            } else {
                // This case indicates an issue with available_tickers list consistency
                warn!("Target symbol index out of bounds during crossover repair.");
                // Fallback: select the first ticker if possible
                if !child_tickers.is_empty() {
                    child_tickers[0] = true;
                }
            }
        } else {
            // This case means child_symbol is not in available_tickers - should not happen if parents are valid
            warn!(
                "Target symbol '{}' not found in available tickers during crossover repair.",
                child_symbol
            );
            // Fallback: select the first ticker if possible
            if !child_tickers.is_empty() {
                child_tickers[0] = true;
            }
        }
        // Ensure at least one technical is selected
        let length = child_technicals.len();
        if !child_technicals.iter().any(|&b| b) && !child_technicals.is_empty() {
            child_technicals[rng.gen_range(0..length)] = true;
        }

        // --- Crossover and Adjust 'n' ---
        // Average n, then clamp based on new feature dimension
        let num_selected_tickers = child_tickers.iter().filter(|&&b| b).count();
        let num_selected_technicals = child_technicals.iter().filter(|&&b| b).count();
        let feature_dimension = child_d * num_selected_tickers * num_selected_technicals;

        // Average parent n values
        let mut child_n = ((parent1.n + parent2.n) / 2).max(1);

        // Clamp child_n: must be >= 1 and <= feature_dimension (if dimension > 0)
        if feature_dimension > 0 {
            child_n = child_n.min(feature_dimension);
        } else {
            child_n = 1; // If dimension is 0, n must be 1 (though this strategy might be invalid)
        }
        child_n = child_n.max(1); // Ensure n is at least 1

        vec![TradingStrategyGenome {
            n: child_n,
            d: child_d,
            interval: child_interval,
            tickers: child_tickers,
            symbol: child_symbol,
            technicals: child_technicals,
        }]
    }
}

// --- Mutation Operator ---
#[derive(Clone, Debug)]
pub struct TradingStrategyMutation {
    mutation_rate: f64,
    available_tickers: Arc<Vec<String>>,
    available_intervals: Arc<Vec<Interval>>,
    // available_technicals: Arc<Vec<String>>, // Needed for repair? No, length comes from genome.
    max_depth: usize,
    max_n: usize,
}

impl TradingStrategyMutation {
    pub fn new(
        mutation_rate: f64,
        available_tickers: Arc<Vec<String>>,
        available_intervals: Arc<Vec<Interval>>,
        // available_technicals: Arc<Vec<String>>,
        max_depth: usize,
        max_n: usize,
    ) -> Self {
        assert!(
            (0.0..=1.0).contains(&mutation_rate),
            "Mutation rate must be between 0.0 and 1.0"
        );
        assert!(
            !available_tickers.is_empty(),
            "Available tickers cannot be empty"
        );
        assert!(
            !available_intervals.is_empty(),
            "Available intervals cannot be empty"
        );
        // assert!(!available_technicals.is_empty(), "Available technicals cannot be empty");
        assert!(max_depth > 0, "Max depth must be > 0");
        assert!(max_n > 0, "Max n must be > 0");
        Self {
            mutation_rate,
            available_tickers,
            available_intervals,
            // available_technicals,
            max_depth,
            max_n,
        }
    }
}

impl genevo::operator::GeneticOperator for TradingStrategyMutation {
    fn name() -> String {
        "TradingStrategyMutation".to_string()
    }
}

impl MutationOp<TradingStrategyGenome> for TradingStrategyMutation {
    fn mutate<R>(&self, mut genome: TradingStrategyGenome, rng: &mut R) -> TradingStrategyGenome
    where
        R: Rng + Sized,
    {
        // Mutate Interval
        if rng.gen_bool(self.mutation_rate) {
            genome.interval = *self
                .available_intervals
                .choose(rng)
                .expect("Intervals non-empty");
        }

        // Mutate Target Symbol
        if rng.gen_bool(self.mutation_rate) {
            genome.symbol = self
                .available_tickers
                .choose(rng)
                .expect("Tickers non-empty")
                .clone();
            // Ensure the new target symbol is selected in the tickers mask
            if let Some(target_idx) = self
                .available_tickers
                .iter()
                .position(|s| s == &genome.symbol)
            {
                if target_idx < genome.tickers.len() {
                    genome.tickers[target_idx] = true;
                }
            }
        }

        // Mutate Tickers Mask (bit-flip)
        let mut num_tickers_selected = 0;
        for ticker_mask_value in genome.tickers.iter_mut() {
            if rng.gen_bool(self.mutation_rate) {
                *ticker_mask_value = !*ticker_mask_value;
            }
            if *ticker_mask_value {
                num_tickers_selected += 1;
            }
        }
        // Repair: Ensure at least one ticker is selected
        if num_tickers_selected == 0 && !genome.tickers.is_empty() {
            let idx = rng.gen_range(0..genome.tickers.len());
            genome.tickers[idx] = true;
            num_tickers_selected = 1;
        }
        // Repair: Ensure target symbol is still selected after mutation
        if let Some(target_idx) = self
            .available_tickers
            .iter()
            .position(|s| s == &genome.symbol)
        {
            if target_idx < genome.tickers.len() && !genome.tickers[target_idx] {
                genome.tickers[target_idx] = true;
                num_tickers_selected += 1; // Increment count if it was flipped off and back on
            }
        }

        // Mutate Technicals Mask (bit-flip)
        let mut num_technicals_selected = 0;
        for technical_mask_value in genome.technicals.iter_mut() {
            if rng.gen_bool(self.mutation_rate) {
                *technical_mask_value = !*technical_mask_value;
            }
            if *technical_mask_value {
                num_technicals_selected += 1;
            }
        }
        // Repair: Ensure at least one technical is selected
        if num_technicals_selected == 0 && !genome.technicals.is_empty() {
            let idx = rng.gen_range(0..genome.technicals.len());
            genome.technicals[idx] = true;
            num_technicals_selected = 1;
        }

        // Mutate Depth (d)
        if rng.gen_bool(self.mutation_rate) {
            genome.d = rng.gen_range(1..=self.max_depth);
        }

        // Mutate Components (n), ensuring validity based on new d, tickers, technicals
        let feature_dimension = genome.d * num_tickers_selected * num_technicals_selected;
        let max_possible_n = feature_dimension.min(self.max_n); // Also cap by config max_n

        if rng.gen_bool(self.mutation_rate) || genome.n > max_possible_n {
            // Mutate n or adjust if current n is now invalid
            genome.n = if max_possible_n > 0 {
                rng.gen_range(1..=max_possible_n)
            } else {
                1 // Default to 1 if feature dimension becomes 0
            };
        }
        // Final clamp for safety
        genome.n = genome.n.max(1);
        if max_possible_n > 0 {
            genome.n = genome.n.min(max_possible_n);
        } else {
            genome.n = 1; // Ensure n=1 if max_possible_n is 0
        }

        genome
    }
}

// --- Report Generation Function ---

/// Generates detailed report files (trade log, equity curve) for a given strategy genome.
/// This function clears the /report/top directory and writes new files.
pub fn generate_trade_log_for_best(
    genome: &TradingStrategyGenome,
    report_dir: &Path, // Base report directory (e.g., "./report")
    config: &KryptoConfig,
    dataset: &Dataset,
    available_tickers: &[String],
    available_technicals: &[String],
) -> Result<(), KryptoError> {
    let top_fitness_path = report_dir.join(TOP_FITNESS_DIR);
    info!(
        "Generating detailed report in '{}' for best strategy found...",
        top_fitness_path.display()
    );

    // 1. Clear the /report/top directory
    if top_fitness_path.exists() {
        fs::remove_dir_all(&top_fitness_path).map_err(|e| {
            KryptoError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to remove dir {}: {}", top_fitness_path.display(), e),
            ))
        })?;
    }
    fs::create_dir_all(&top_fitness_path).map_err(|e| {
        KryptoError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to create dir {}: {}", top_fitness_path.display(), e),
        ))
    })?;

    // 2. Convert genome to phenotype
    let strategy = genome.to_phenotype(available_tickers, available_technicals)?;
    info!("Strategy for detailed report: {}", strategy);

    // 3. Get relevant data and settings
    let base_interval_data = dataset
        .get(&strategy.interval)
        .ok_or_else(|| KryptoError::IntervalNotFound(strategy.interval.to_string()))?;

    // Filter data (recomputes technicals/normalization)
    let specific_interval_data = base_interval_data
        .get_specific_tickers_and_technicals(&strategy.tickers, &strategy.technicals)?;

    let settings = AlgorithmSettings::from(&strategy);

    // 4. Get the full dataset for this specific strategy configuration
    // This dataset is used for both training the final model and running the simulation
    let full_symbol_dataset = specific_interval_data.get_symbol_dataset(&settings)?;
    if full_symbol_dataset.is_empty() {
        warn!("Dataset for {} is empty, cannot generate report.", strategy);
        return Ok(()); // Or return error? Ok allows optimisation to continue.
    }
    let features = full_symbol_dataset.get_features();
    let labels = full_symbol_dataset.get_labels(); // Use raw labels for PLS training
    let candles = full_symbol_dataset.get_candles(); // Candles aligned with features/labels

    // 5. Train the *final* model on the *entire* filtered dataset
    info!("Training final model on full data for report generation...");
    // Validate data before final fit (copied from algo.rs)
    if features
        .iter()
        .flatten()
        .any(|&x| x.is_nan() || x.is_infinite())
    {
        return Err(KryptoError::PlsInternalError(
            "NaN or Infinity detected in predictor data for report generation.".to_string(),
        ));
    }
    if labels.iter().any(|&x| x.is_nan() || x.is_infinite()) {
        return Err(KryptoError::PlsInternalError(
            "NaN or Infinity detected in target data for report generation.".to_string(),
        ));
    }

    let final_pls = match get_pls(features, labels, settings.n) {
        Ok(model) => model,
        Err(e) => {
            error!("Failed to train final PLS model for report generation: {}", e);
            return Err(e); // Propagate error
        }
    };
    info!("Final model trained.");

    // 6. Predict on the entire dataset using the final model
    let predictions = match predict(&final_pls, features) {
        Ok(preds) => preds,
        Err(e) => {
            error!("Failed to predict using final PLS model for report generation: {}", e);
            return Err(e); // Propagate error
        }
    };

    // 7. Run the simulation using TestData::run_simulation on the full dataset
    // This gives the performance metrics, trade log, and equity curve for the *entire period*
    info!("Running final simulation for report generation...");
    let simulation_output: SimulationOutput = match TestData::run_simulation(
        &settings.symbol,
        &predictions,
        candles, // Pass the candles aligned with features/labels/predictions
        config,
    ) {
        Ok(output) => output,
        Err(e) => {
            error!("Failed to run final simulation for report generation: {}", e);
            return Err(e); // Propagate error
        }
    };
    info!(
        "Final simulation complete. Result on full data: {}",
        simulation_output.metrics
    );

    // 8. Write trade log to CSV
    // Use a fixed name like "top-strategy-trades.csv" since we clear the dir
    let log_file_path = top_fitness_path.join("top-strategy-trades.csv");
    match write_trade_log_csv(&log_file_path, &simulation_output.trade_log) {
        Ok(_) => info!("Trade log written to {}", log_file_path.display()),
        Err(e) => error!("Failed to write trade log CSV: {}", e), // Log error but continue
    }

    // 9. Write equity curve to separate CSV
    let equity_file_path = top_fitness_path.join("top-strategy-equity.csv");
    match write_equity_curve_csv(&equity_file_path, &simulation_output.equity_curve) {
        Ok(_) => info!("Equity curve written to {}", equity_file_path.display()),
        Err(e) => error!("Failed to write equity curve CSV: {}", e), // Log error
    }

    Ok(())
}

// Helper function to write trade log CSV
fn write_trade_log_csv(
    path: &Path,
    trade_log: &[crate::algorithm::test_data::TradeLogEntry],
) -> Result<(), KryptoError> {
    let mut writer = csv::Writer::from_path(path)?;
    // Write header matching TradeLogEntry field names (ensure order matches struct/desired output)
    // Apply clippy suggestion: remove &
    writer.write_record([
        "timestamp",
        "symbol",
        "side",
        "entry_price",
        "exit_price",
        "quantity",
        "pnl",
        "pnl_pct",
        "fee",
        "cash_after_trade",
        "equity_after_trade",
        "reason",
    ])?;
    for entry in trade_log {
        let side_str = match entry.side { // Manual serialization for CSV
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        };
        writer.write_record(&[
            entry.timestamp.to_rfc3339(),
            entry.symbol.clone(),
            side_str.to_string(),
            format!("{:.8}", entry.entry_price), // Adjust precision as needed
            format!("{:.8}", entry.exit_price),
            format!("{:.8}", entry.quantity),
            format!("{:.8}", entry.pnl),
            format!("{:.6}", entry.pnl_pct),
            format!("{:.8}", entry.fee),
            format!("{:.2}", entry.cash_after_trade),
            format!("{:.2}", entry.equity_after_trade),
            entry.reason.clone(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

// Helper function to write equity curve CSV
fn write_equity_curve_csv(
    path: &Path,
    equity_curve: &[(chrono::DateTime<chrono::Utc>, f64)],
) -> Result<(), KryptoError> {
    let mut writer = csv::Writer::from_path(path)?;
    // Apply clippy suggestion: remove &
    writer.write_record(["timestamp", "equity"])?;
    for (time, equity) in equity_curve {
        writer.write_record(&[time.to_rfc3339(), format!("{:.2}", equity)])?;
    }
    writer.flush()?;
    Ok(())
}