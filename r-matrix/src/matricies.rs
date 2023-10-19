use std::io::{Write, Read};
use std::num::NonZeroUsize;

use crate::data::{RData, RDataEntry, RMatrixId};
use crate::errors::RError;
use crate::math::{bayes_combine, NormalizationFunctionType};
use crate::relationship::{Relationship, RelationshipEntry};
use derive_builder::Builder;
use getset::Getters;
use randomforest::criterion::Mse;
use randomforest::table::{TableBuilder, TableError};
use randomforest::{RandomForestRegressor, RandomForestRegressorOptions};
use serde::{Deserialize, Serialize};

pub trait RMatrix<T> {
    /// Calculate the score for the given data at the given index.
    fn predict(&self, data: &RData<T>, index: usize) -> f64;
    /// A more stable version of `predict` that returns an error if the data is invalid.
    fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError>;
}

#[derive(Debug, Getters, Serialize, Deserialize)]
#[getset(get = "pub")]
/// A struct that represents the RMatrix.
pub struct SimpleRMatrix<T> {
    /// The list of relationships between the target and record entries.
    /// The list is ordered by record entry and then by depth.
    relationships: Box<[RelationshipEntry]>,
    /// The maximum depth of the RMatrix.
    max_depth: usize,
    /// The normalization function to use for the RMatrix. Default is `tanh`.
    function: NormalizationFunctionType,
    #[serde(skip)]
    _phantom: std::marker::PhantomData<T>,
}

impl<T: RMatrixId> SimpleRMatrix<T> {
    pub fn new(max_depth: usize, function: NormalizationFunctionType) -> Self {
        Self {
            relationships: Box::new([]),
            max_depth,
            function,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Calculate the relationships between the target and record entries.
    /// This function will overwrite any existing relationships.
    pub async fn calculate_relationships(&mut self, data: &RData<T>) {
        let target = data.target();
        let records = data.records();
        let tasks = records
            .iter()
            .map(|record| self.compute(record, target))
            .collect::<Vec<_>>();
        let relationships = futures::future::join_all(tasks)
            .await
            .into_iter()
            .collect::<Vec<_>>();
        self.relationships = relationships.into_boxed_slice();
    }

    async fn compute(&self, record: &RDataEntry<T>, target: &RDataEntry<T>) -> RelationshipEntry {
        let max_depth = self.max_depth;
        let length = record.data().len();
        let mut results = vec![Vec::new(); max_depth];
        for i in max_depth..length.saturating_sub(1) {
            let target_value = target.data().get(i + 1).unwrap_or(&0.0);
            for depth in 0..max_depth {
                let record_value = record.data().get(i - depth).unwrap_or(&0.0);
                let result = self.function.get_function()(*record_value * *target_value);
                results[depth].push(result);
            }
        }
        let results = results
            .into_iter()
            .enumerate()
            .map(|(depth, values)| Relationship::new(values, depth + 1));
        RelationshipEntry::new(results.collect())
    }

    #[inline]
    pub async fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        if data.records().len() != self.relationships.len() {
            return Err(RError::RelationshipRecordCountMismatchError(
                data.records().len(),
                self.relationships.len(),
            ));
        }
        let mut prior = *data.prior();
        for (i, entry) in self.relationships.iter().enumerate() {
            for relationship in entry.relationships().iter() {
                let record = data.records().get(i);
                if record.is_none() {
                    return Err(RError::RecordIndexOutOfBoundsError {
                        index: i,
                        length: data.records().len(),
                    });
                }
                let record = record.unwrap().data().get(index - relationship.depth);
                if record.is_none() {
                    return Err(RError::RecordIndexOutOfBoundsError {
                        index: index - relationship.depth,
                        length: data.records()[i].data().len(),
                    });
                }
                let predicted = record.unwrap() * data.mean();
                prior = bayes_combine(prior, Self::calculate_probability(relationship, predicted));
            }
        }
        Ok(prior)
    }

    #[inline(always)]
    pub async fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        let mut prior = *data.prior();
        for (i, entry) in self.relationships.iter().enumerate() {
            let record = &data.records()[i];
            for relationship in entry.relationships().iter() {
                let predicted = record.data()[index - relationship.depth] * data.mean();
                prior = bayes_combine(prior, Self::calculate_probability(relationship, predicted));
            }
        }
        prior
    }

    #[inline(always)]
    fn calculate_probability(relationship: &Relationship, predicted: f64) -> f64 {
        let z_score = (0.0 - predicted) / relationship.standard_deviation();
        crate::math::norm_s_dist(z_score)
    }
}

impl<T: RMatrixId> RMatrix<T> for SimpleRMatrix<T> {
    fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        futures::executor::block_on(self.predict(data, index))
    }

    fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        futures::executor::block_on(self.predict_stable(data, index))
    }
}

#[derive(Debug, Getters, Builder)]
#[getset(get = "pub")]
pub struct ForestConfig {
    depth: usize,
    trees: usize,
    seed: u64,
    ending_position: usize,
    max_samples: usize,
}

#[derive(Debug, Getters)]
#[getset(get = "pub")]
pub struct ForestRMatrix<T> {
    /// The underlying random forest regressor.
    regressor: RandomForestRegressor,
    /// The depth of the RMatrix.
    depth: usize,
    /// The phantom data.
    _phantom: std::marker::PhantomData<T>,
}

impl<T: RMatrixId> ForestRMatrix<T> {
    pub fn new(data: &RData<T>, config: ForestConfig) -> Result<Self, TableError> {
        let targets = data.target();
        let features = data.transpose_records();
        let mut table_builder = TableBuilder::new();
        for i in config.depth..config.ending_position {
            let target = targets.data()[i];
            let mut predictors: Vec<f64> = Vec::new();
            for d in 0..config.depth {
                predictors.extend(features[i - d - 1].iter());
            }
            table_builder.add_row(&predictors, target)?;
        }
        let table = table_builder.build()?;
        let regressor = RandomForestRegressorOptions::new()
            .seed(config.seed)
            .trees(NonZeroUsize::new(config.trees).unwrap())
            .max_samples(NonZeroUsize::new(config.max_samples).unwrap())
            .fit(Mse, table);
        Ok(Self {
            regressor,
            depth: config.depth,
            _phantom: std::marker::PhantomData,
        })
    }

    #[inline(always)]
    pub fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        let features: Vec<_> = (0..self.depth)
            .flat_map(|i| {
                data.records()
                    .iter()
                    .map(move |record| record.data()[index - i - 1])
            })
            .collect();
        self.regressor.predict(&features)
    }

    #[inline]
    pub fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        let mut features = Vec::with_capacity(self.depth * data.records().len());
        for i in 0..self.depth {
            for record in data.records().iter() {
                let result = record.data().get(index - i - 1);
                match result {
                    Some(result) => features.push(*result),
                    None => {
                        return Err(RError::RecordIndexOutOfBoundsError {
                            index: index - i - 1,
                            length: record.data().len(),
                        })
                    }
                }
            }
        }
        Ok(self.regressor.predict(&features))
    }

    pub fn serialize<W: Write>(&self, writer: W) -> std::io::Result<()> {
        self.regressor.serialize(writer)
    }

    pub fn deserialize<R: Read>(reader: R, depth: usize) -> std::io::Result<Self> {
        let regressor = RandomForestRegressor::deserialize(reader)?;
        Ok(Self {
            regressor,
            depth,
            _phantom: std::marker::PhantomData,
        })
    }


}

impl<T: RMatrixId> RMatrix<T> for ForestRMatrix<T> {
    fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        self.predict(data, index)
    }

    fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        self.predict_stable(data, index)
    }
}
