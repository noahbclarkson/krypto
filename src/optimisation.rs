use std::{fmt, sync::Arc};

use genevo::{
    genetic::{Children, Parents},
    operator::{CrossoverOp, MutationOp},
    prelude::{FitnessFunction, GenomeBuilder, Genotype},
    random::{Rng, SliceRandom as _},
};
use tracing::debug;

use crate::{
    algorithm::algo::{Algorithm, AlgorithmSettings},
    config::KryptoConfig,
    data::{dataset::Dataset, interval::Interval, technicals::TECHNICAL_COUNT},
};

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct TradingStrategy {
    n: usize,
    d: usize,
    interval: Interval,
    tickers: Vec<String>,
    symbol: String,
}

impl Genotype for TradingStrategy {
    type Dna = Self;
}

impl TradingStrategy {
    pub fn new(
        n: usize,
        d: usize,
        interval: Interval,
        tickers: Vec<String>,
        symbol: String,
    ) -> Self {
        Self {
            n,
            d,
            interval,
            tickers,
            symbol,
        }
    }
}

impl fmt::Display for TradingStrategy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "TradingStrategy: n={}, d={}, interval={}, symbol={}, tickers={}",
            self.n,
            self.d,
            self.interval,
            self.symbol,
            self.tickers.join(",")
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
    max_depth: usize,
    max_n: usize,
}

impl TradingStrategyGenomeBuilder {
    pub fn new(
        available_tickers: Vec<String>,
        available_intervals: Vec<Interval>,
        max_depth: usize,
        max_n: usize,
    ) -> Self {
        Self {
            available_tickers,
            available_intervals,
            max_depth,
            max_n,
        }
    }

    fn tickers<R>(&self, r: &mut R) -> (Vec<String>, String)
    where
        R: Rng + Sized,
    {
        let num = r.gen_range(1..=self.available_tickers.len());
        let tickers: Vec<String> = self
            .available_tickers
            .choose_multiple(r, num)
            .cloned()
            .collect();
        let symbol = tickers.choose(r).unwrap().clone();
        (tickers, symbol)
    }
}

impl GenomeBuilder<TradingStrategy> for TradingStrategyGenomeBuilder {
    fn build_genome<R>(&self, _: usize, rng: &mut R) -> TradingStrategy
    where
        R: Rng + Sized,
    {
        let (tickers, symbol) = self.tickers(rng);
        let depth = rng.gen_range(1..=self.max_depth);
        let max_n = depth * tickers.len() * TECHNICAL_COUNT - 1;
        let n = rng.gen_range(1..=max_n.min(self.max_n));
        let interval = *self.available_intervals.choose(rng).unwrap();
        TradingStrategy::new(n, depth, interval, tickers, symbol)
    }
}

#[derive(Clone)]
pub struct TradingStrategyFitnessFunction {
    config: Arc<KryptoConfig>,
    dataset: Arc<Dataset>,
}

impl fmt::Debug for TradingStrategyFitnessFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TradingStrategyFitnessFunction")
    }
}

impl TradingStrategyFitnessFunction {
    pub fn new(config: Arc<KryptoConfig>, dataset: Arc<Dataset>) -> Self {
        Self { config, dataset }
    }
}

impl FitnessFunction<TradingStrategy, i64> for TradingStrategyFitnessFunction {
    #[tracing::instrument(skip(self))]
    fn fitness_of(&self, a: &TradingStrategy) -> i64 {
        let data = self.dataset.get(&a.interval).unwrap();
        let data = data.get_specific_tickers(&a.tickers);
        let settings = AlgorithmSettings::from(a.clone());
        let algorithm = Algorithm::load(&data, settings, &self.config).unwrap();
        let monthly_return = algorithm.get_monthly_return();
        if monthly_return.is_nan() || monthly_return.is_infinite() {
            return i64::MIN;
        }
        debug!("Evaluated fitness: {:.2}%", monthly_return * 100.0);
        (algorithm.get_monthly_return() * 10_000.0) as i64
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
pub struct TradingStrategyCrossover;

impl genevo::operator::GeneticOperator for TradingStrategyCrossover {
    fn name() -> String {
        "TradingStrategyCrossover".to_string()
    }
}

impl CrossoverOp<TradingStrategy> for TradingStrategyCrossover {
    fn crossover<R>(
        &self,
        parents: Parents<TradingStrategy>,
        rng: &mut R,
    ) -> Children<TradingStrategy>
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

        // Crossover tickers
        let mut child_tickers = parent1.tickers.clone();
        child_tickers.extend(parent2.tickers.clone());
        child_tickers.sort();
        child_tickers.dedup();

        let child_d = if rng.gen_bool(0.5) {
            parent1.d
        } else {
            parent2.d
        };
        let mut child_n = if rng.gen_bool(0.5) {
            parent1.n
        } else {
            parent2.n
        };
        if child_n > child_d * child_tickers.len() * TECHNICAL_COUNT - 1 {
            child_n = child_d * child_tickers.len() * TECHNICAL_COUNT - 1;
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
        if !child_tickers.contains(&child_symbol) {
            child_tickers.push(child_symbol.clone());
        }

        let child = TradingStrategy {
            n: child_n,
            d: child_d,
            interval: child_interval,
            tickers: child_tickers,
            symbol: child_symbol,
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

impl MutationOp<TradingStrategy> for TradingStrategyMutation {
    fn mutate<R>(&self, genome: TradingStrategy, rng: &mut R) -> TradingStrategy
    where
        R: Rng + Sized,
    {
        let mut new_genome = genome.clone();
        if rng.gen_bool(self.mutation_rate) {
            let (tickers, symbol) = TradingStrategyGenomeBuilder::new(
                self.available_tickers.clone(),
                self.available_intervals.clone(),
                self.max_depth,
                self.max_n,
            )
            .tickers(rng);
            new_genome.tickers = tickers;
            new_genome.symbol = symbol;
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.d = rng.gen_range(1..=self.max_depth);
        }

        if rng.gen_bool(self.mutation_rate) {
            let max_n = new_genome.d * new_genome.tickers.len() * TECHNICAL_COUNT - 1;
            new_genome.n = rng.gen_range(1..=max_n.min(self.max_n));
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.interval = *self.available_intervals.choose(rng).unwrap();
        }

        new_genome
    }
}
