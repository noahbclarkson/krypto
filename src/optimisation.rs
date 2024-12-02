use std::{
    collections::HashMap,
    fmt, panic,
    sync::{Arc, Mutex},
};

use genevo::{
    genetic::{Children, Parents},
    operator::{CrossoverOp, MutationOp},
    prelude::{FitnessFunction, GenomeBuilder, Genotype},
    random::{Rng, SliceRandom as _},
};
use tracing::{debug, info};

use crate::{
    algorithm::algo::{Algorithm, AlgorithmSettings},
    config::KryptoConfig,
    data::{dataset::Dataset, interval::Interval},
};

#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Eq)]
pub struct TradingStrategyGenome {
    n: usize,
    d: usize,
    interval: Interval,
    tickers: Vec<bool>,
    symbol: String,
    technicals: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct TradingStrategy {
    n: usize,
    d: usize,
    interval: Interval,
    tickers: Vec<String>,
    symbol: String,
    technicals: Vec<String>,
}

impl Genotype for TradingStrategyGenome {
    type Dna = Self;
}

impl TradingStrategyGenome {
    pub fn to_phenotype(
        &self,
        available_tickers: &[String],
        available_technicals: &[String],
    ) -> TradingStrategy {
        let tickers = self
            .tickers
            .iter()
            .zip(available_tickers.iter())
            .filter_map(|(b, s)| if *b { Some(s.clone()) } else { None })
            .collect();
        let technicals = self
            .technicals
            .iter()
            .zip(available_technicals.iter())
            .filter_map(|(b, s)| if *b { Some(s.clone()) } else { None })
            .collect();
        TradingStrategy::new(
            self.n,
            self.d,
            self.interval,
            tickers,
            self.symbol.clone(),
            technicals,
        )
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
            "TradingStrategy: n={}, d={}, interval={}, symbol={}, tickers={}, technicals={}",
            self.n,
            self.d,
            self.interval,
            self.symbol,
            self.tickers.join(","),
            self.technicals.join(",")
        )
    }
}

impl From<TradingStrategy> for AlgorithmSettings {
    fn from(a: TradingStrategy) -> Self {
        Self::new(a.n, a.d, &a.symbol)
    }
}

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
        Self {
            available_tickers,
            available_intervals,
            available_technicals,
            max_depth,
            max_n,
        }
    }

    fn tickers<R>(&self, r: &mut R) -> (Vec<bool>, String)
    where
        R: Rng + Sized,
    {
        let num_selected = r.gen_range(1..=self.available_tickers.len());
        let mut tickers = vec![false; self.available_tickers.len()];
        for _ in 0..num_selected {
            let pos = r.gen_range(0..self.available_tickers.len());
            tickers[pos] = true;
        }
        // Ensure the symbol is included
        let symbol = self.available_tickers.choose(r).unwrap().clone();
        let pos_of_symbol = self
            .available_tickers
            .iter()
            .position(|s| s == &symbol)
            .unwrap();
        tickers[pos_of_symbol] = true;
        (tickers, symbol)
    }

    fn technicals<R>(&self, r: &mut R) -> Vec<bool>
    where
        R: Rng + Sized,
    {
        let num_selected = r.gen_range(1..=self.available_technicals.len());
        let mut technicals = vec![false; self.available_technicals.len()];
        for _ in 0..num_selected {
            let pos = r.gen_range(0..self.available_technicals.len());
            technicals[pos] = true;
        }
        technicals
    }
}

impl GenomeBuilder<TradingStrategyGenome> for TradingStrategyGenomeBuilder {
    fn build_genome<R>(&self, _: usize, rng: &mut R) -> TradingStrategyGenome
    where
        R: Rng + Sized,
    {
        let (tickers, symbol) = self.tickers(rng);
        let depth = rng.gen_range(1..=self.max_depth);
        let technicals = self.technicals(rng);
        let technical_count = technicals.iter().filter(|b| **b).count();
        let tickers_count = tickers.iter().filter(|b| **b).count();
        let max_n = depth * tickers_count * technical_count;
        let n = rng.gen_range(1..=max_n.min(self.max_n));
        let interval = *self.available_intervals.choose(rng).unwrap();
        TradingStrategyGenome {
            n,
            d: depth,
            interval,
            tickers,
            symbol,
            technicals,
        }
    }
}

#[derive(Clone)]
pub struct TradingStrategyFitnessFunction {
    config: Arc<KryptoConfig>,
    dataset: Arc<Dataset>,
    available_tickers: Vec<String>,
    available_technicals: Vec<String>,
    fitness_cache: Arc<Mutex<HashMap<TradingStrategyGenome, i64>>>,
}

impl fmt::Debug for TradingStrategyFitnessFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TradingStrategyFitnessFunction")
    }
}

impl TradingStrategyFitnessFunction {
    pub fn new(
        config: Arc<KryptoConfig>,
        dataset: Arc<Dataset>,
        available_tickers: Vec<String>,
        available_technicals: Vec<String>,
    ) -> Self {
        let fitness_cache = Arc::new(Mutex::new(HashMap::new()));
        Self {
            config,
            dataset,
            available_tickers,
            available_technicals,
            fitness_cache,
        }
    }

    pub fn clear_cache(&self) {
        let mut cache = self.fitness_cache.lock().unwrap();
        cache.clear();
    }

    pub fn get_cache(&self) -> std::sync::MutexGuard<HashMap<TradingStrategyGenome, i64>> {
        self.fitness_cache.lock().unwrap()
    }

    pub fn cache(&self, genome: TradingStrategyGenome, fitness: i64) {
        let mut cache = self.fitness_cache.lock().unwrap();
        cache.insert(genome, fitness);
    }

    pub fn cache_contains(&self, genome: &TradingStrategyGenome) -> bool {
        let cache = self.fitness_cache.lock().unwrap();
        cache.contains_key(genome)
    }
}

impl FitnessFunction<TradingStrategyGenome, i64> for TradingStrategyFitnessFunction {
    #[tracing::instrument(skip(self, a))]
    fn fitness_of(&self, a: &TradingStrategyGenome) -> i64 {
        if self.cache_contains(a) {
            return *self.get_cache().get(a).unwrap();
        }

        let strategy = a.to_phenotype(&self.available_tickers, &self.available_technicals);
        debug!("Evaluating fitness of strategy: {}", strategy);
        let data = self.dataset.get(&a.interval).unwrap();
        let data = panic::catch_unwind(|| {
            data.get_specific_tickers_and_technicals(&strategy.tickers, &strategy.technicals)
        });
        let data = match data {
            Ok(data) => data,
            Err(e) => {
                info!("Failed to get data: {} with error: {:?}", strategy, e);
                return i64::MIN;
            }
        };
        let settings = AlgorithmSettings::from(strategy.clone());
        let algorithm = Algorithm::load(&data, settings, &self.config);
        match algorithm {
            Ok(_) => {}
            Err(e) => {
                info!("Failed to evaluate fitness: {} with error: {}", strategy, e);
                return i64::MIN;
            }
        }
        let algorithm = algorithm.unwrap();
        let monthly_return = algorithm.get_monthly_return();
        if monthly_return.is_nan() || monthly_return.is_infinite() {
            return i64::MIN;
        }
        debug!(
            "Evaluated fitness of strategy {}: {:.2}%",
            strategy,
            monthly_return * 100.0
        );
        let fitness = (monthly_return * 10_000.0) as i64;
        self.cache(a.clone(), fitness);
        fitness
    }

    fn average(&self, a: &[i64]) -> i64 {
        a.iter().sum::<i64>() / a.len() as i64
    }

    fn highest_possible_fitness(&self) -> i64 {
        i64::MAX
    }

    fn lowest_possible_fitness(&self) -> i64 {
        i64::MIN
    }
}

#[derive(Clone, Debug)]
pub struct TradingStrategyCrossover {
    pub available_tickers: Vec<String>,
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
        if parents.len() != 2 {
            panic!(
                "TradingStrategyCrossover requires exactly 2 parents, got {}",
                parents.len()
            );
        }
        let parent1 = &parents[0];
        let parent2 = &parents[1];

        // Crossover tickers by splitting tickers in half and merging them (either parent1 or parent2 first)
        let mut child_tickers = Vec::new();
        for (t1, t2) in parent1.tickers.iter().zip(parent2.tickers.iter()) {
            if rng.gen_bool(0.5) {
                child_tickers.push(*t1);
            } else {
                child_tickers.push(*t2);
            }
        }

        let mut child_technicals = Vec::new();
        for (t1, t2) in parent1.technicals.iter().zip(parent2.technicals.iter()) {
            if rng.gen_bool(0.5) {
                child_technicals.push(*t1);
            } else {
                child_technicals.push(*t2);
            }
        }
        if child_technicals.iter().all(|&b| !b) {
            let index = rng.gen_range(0..child_technicals.len());
            child_technicals[index] = true;
        }

        let tech_count = child_technicals.iter().filter(|b| **b).count();
        let tickers_count = child_tickers.iter().filter(|b| **b).count();
        let child_d = (parent1.d + parent2.d) / 2;
        let mut child_n = (parent1.n + parent2.n) / 2;
        if child_n > child_d * tickers_count * tech_count {
            child_n = child_d * tickers_count * tech_count;
        }

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
        let pos_of_symbol = self
            .available_tickers
            .iter()
            .position(|s| s == &child_symbol)
            .unwrap();
        child_tickers[pos_of_symbol] = true;

        let child = TradingStrategyGenome {
            n: child_n,
            d: child_d,
            interval: child_interval,
            tickers: child_tickers,
            symbol: child_symbol,
            technicals: child_technicals,
        };

        vec![child]
    }
}

#[derive(Clone, Debug)]
pub struct TradingStrategyMutation {
    mutation_rate: f64,
    available_tickers: Vec<String>,
    available_intervals: Vec<Interval>,
    max_depth: usize,
    max_n: usize,
}

impl TradingStrategyMutation {
    pub fn new(
        mutation_rate: f64,
        available_tickers: Vec<String>,
        available_intervals: Vec<Interval>,
        max_depth: usize,
        max_n: usize,
    ) -> Self {
        Self {
            mutation_rate,
            available_tickers,
            available_intervals,
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
    fn mutate<R>(&self, genome: TradingStrategyGenome, rng: &mut R) -> TradingStrategyGenome
    where
        R: Rng + Sized,
    {
        let mut new_genome = genome.clone();

        for ticker in new_genome.tickers.iter_mut() {
            if rng.gen_bool(self.mutation_rate) {
                *ticker = !*ticker;
            }
        }

        // Ensure symbol is still included
        let pos_of_symbol = self
            .available_tickers
            .iter()
            .position(|s| s == &new_genome.symbol)
            .unwrap();
        new_genome.tickers[pos_of_symbol] = true;

        // Mutate individual bits in technicals
        for technical in new_genome.technicals.iter_mut() {
            if rng.gen_bool(self.mutation_rate) {
                *technical = !*technical;
            }
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.d = rng.gen_range(1..=self.max_depth);
        }

        let tech_count = new_genome.technicals.iter().filter(|b| **b).count();
        let tickers_count = new_genome.tickers.iter().filter(|b| **b).count();
        if rng.gen_bool(self.mutation_rate) {
            let max_n = new_genome.d * tickers_count * tech_count - 1;
            new_genome.n = rng.gen_range(1..=max_n.min(self.max_n));
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.interval = *self.available_intervals.choose(rng).unwrap();
        }

        new_genome
    }
}
