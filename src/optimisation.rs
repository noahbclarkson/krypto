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
    data::{dataset::Dataset, interval::Interval},
};

#[derive(Clone, Debug, PartialEq, PartialOrd)]
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
        let n = self.available_tickers.len();
        let mut tickers = Vec::new();
        for _ in 0..n {
            tickers.push(r.gen_bool(0.25));
        }
        let symbol = self.available_tickers.choose(r).unwrap().clone();
        (tickers, symbol)
    }

    fn technicals<R>(&self, r: &mut R) -> Vec<bool>
    where
        R: Rng + Sized,
    {
        let n = self.available_technicals.len();
        let mut technicals = Vec::new();
        for _ in 0..n {
            technicals.push(r.gen_bool(0.5));
        }
        technicals
    }
}

impl GenomeBuilder<TradingStrategyGenome> for TradingStrategyGenomeBuilder {
    fn build_genome<R>(&self, _: usize, rng: &mut R) -> TradingStrategyGenome
    where
        R: Rng + Sized,
    {
        let (mut tickers, symbol) = self.tickers(rng);
        let pos_of_symbol = self
            .available_tickers
            .iter()
            .position(|s| s == &symbol)
            .unwrap();
        tickers[pos_of_symbol] = true;
        let depth = rng.gen_range(1..=self.max_depth);
        let technicals = self.technicals(rng);
        let technical_count = technicals.iter().filter(|b| **b).count();
        let max_n = depth * tickers.len() * technical_count - 1;
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
        Self {
            config,
            dataset,
            available_tickers,
            available_technicals,
        }
    }

    pub fn to_phenotype(&self, genome: &TradingStrategyGenome) -> TradingStrategy {
        let tickers = genome
            .tickers
            .iter()
            .zip(self.available_tickers.iter())
            .filter_map(|(b, s)| if *b { Some(s.clone()) } else { None })
            .collect();
        let technicals = genome
            .technicals
            .iter()
            .zip(self.available_technicals.iter())
            .filter_map(|(b, s)| if *b { Some(s.clone()) } else { None })
            .collect();
        TradingStrategy::new(
            genome.n,
            genome.d,
            genome.interval,
            tickers,
            genome.symbol.clone(),
            technicals,
        )
    }
}

impl FitnessFunction<TradingStrategyGenome, i64> for TradingStrategyFitnessFunction {
    #[tracing::instrument(skip(self))]
    fn fitness_of(&self, a: &TradingStrategyGenome) -> i64 {
        let strategy = self.to_phenotype(a);
        debug!("Evaluating fitness of strategy: {}", strategy);
        let data = self.dataset.get(&a.interval).unwrap();
        let data =
            data.get_specific_tickers_and_technicals(&strategy.tickers, &strategy.technicals);
        let settings = AlgorithmSettings::from(strategy.clone());
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

        let tech_count = child_technicals.iter().filter(|b| **b).count();
        let child_d = (parent1.d as f64 * 0.5 + parent2.d as f64 * 0.5) as usize;
        let mut child_n = (parent1.n as f64 * 0.5 + parent2.n as f64 * 0.5) as usize;
        if child_n > child_d * child_tickers.len() * tech_count - 1 {
            child_n = child_d * child_tickers.len() * tech_count - 1;
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
        if rng.gen_bool(self.mutation_rate) {
            let (tickers, symbol) = TradingStrategyGenomeBuilder::new(
                self.available_tickers.clone(),
                self.available_intervals.clone(),
                self.available_tickers.clone(),
                self.max_depth,
                self.max_n,
            )
            .tickers(rng);
            new_genome.tickers = tickers;
            new_genome.symbol = symbol;
            let pos_of_symbol = self
                .available_tickers
                .iter()
                .position(|s| s == &new_genome.symbol)
                .unwrap();
            new_genome.tickers[pos_of_symbol] = true;
        }

        if rng.gen_bool(self.mutation_rate) {
            let technicals = TradingStrategyGenomeBuilder::new(
                self.available_tickers.clone(),
                self.available_intervals.clone(),
                self.available_tickers.clone(),
                self.max_depth,
                self.max_n,
            )
            .technicals(rng);
            new_genome.technicals = technicals;
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.d = rng.gen_range(1..=self.max_depth);
        }

        let tech_count = new_genome.technicals.iter().filter(|b| **b).count();
        if rng.gen_bool(self.mutation_rate) {
            let max_n = new_genome.d * new_genome.tickers.len() * tech_count - 1;
            new_genome.n = rng.gen_range(1..=max_n.min(self.max_n));
        }

        if rng.gen_bool(self.mutation_rate) {
            new_genome.interval = *self.available_intervals.choose(rng).unwrap();
        }

        new_genome
    }
}
