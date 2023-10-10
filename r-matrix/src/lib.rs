use errors::RError;
use getset::Getters;
use math::NormalizationFunctionType;
use relationship::{Relationship, RelationshipEntry};
use serde::{Deserialize, Serialize};

pub mod errors;
pub mod math;
mod relationship;
mod test;

#[derive(Debug, Getters, Serialize, Deserialize)]
#[getset(get = "pub")]
/// A struct that represents the RMatrix.
pub struct RMatrix<T> {
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

impl<T: RMatrixId> RMatrix<T> {
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
        let target = &data.target;
        let records = &data.records;
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
        let length = record.data.len();
        let mut results = vec![Vec::new(); max_depth];
        for i in max_depth..length.saturating_sub(1) {
            let target_value = target.data.get(i + 1).unwrap_or(&0.0);
            for depth in 0..max_depth {
                let record_value = record.data.get(i - depth).unwrap_or(&0.0);
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
    /// A variant of the predict function that checks that all data is valid before predicting.
    pub async fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        if data.records.len() != self.relationships.len() {
            return Err(RError::RelationshipRecordCountMismatchError(
                data.records.len(),
                self.relationships.len(),
            ));
        }
        let mut prior = data.prior;
        for (i, entry) in self.relationships.iter().enumerate() {
            for relationship in entry.relationships().iter() {
                let record = data.records().get(i);
                if record.is_none() {
                    return Err(RError::RecordIndexOutOfBoundsError {
                        index: i,
                        length: data.records().len(),
                    });
                }
                let record = record.unwrap().data.get(index - relationship.depth);
                if record.is_none() {
                    return Err(RError::RecordIndexOutOfBoundsError {
                        index: index - relationship.depth,
                        length: data.records()[i].data.len(),
                    });
                }
                let predicted = record.unwrap() * data.mean;
                prior = math::bayes_combine(prior, self.calculate_probability(relationship, predicted));
            }
        }
        Ok(prior)
    }

    #[inline(always)]
    /// Calculate the score for the given data at the given index.
    pub async fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        let mut prior = data.prior;
        for (i, entry) in self.relationships.iter().enumerate() {
            let record = &data.records()[i];
            for relationship in entry.relationships().iter() {
                let predicted = record.data[index - relationship.depth] * data.mean;
                prior = math::bayes_combine(prior, self.calculate_probability(relationship, predicted));
            }
        }
        prior
    }

    #[inline(always)]
    fn calculate_probability(&self, relationship: &Relationship, predicted: f64) -> f64 {
        let z_score = (0.0 - predicted) / relationship.standard_deviation();
        math::norm_s_dist(z_score)
    }
}

/// A trait that defines the required functions for an RMatrix identity.
pub trait RMatrixId {
    /// Get the id of the RMatrix identity.
    fn get_id(&self) -> &str;
    /// Check if the RMatrix identity is the target.
    fn is_target(&self) -> bool;
}

#[derive(Debug, Getters)]
#[getset(get = "pub")]
/// The RMatrix dataset contains the data required to calculate the RMatrix.
pub struct RData<T> {
    /// The list of record entries in the RMatrix dataset.
    records: Vec<RDataEntry<T>>,
    /// The prediction entry in the RMatrix dataset.
    target: RDataEntry<T>,
    /// The mean of the targets
    mean: f64,
    /// The standard deviation of the targets
    standard_deviation: f64,
    /// The probability that the target will be positive.
    prior: f64,
}

impl<T: RMatrixId> RData<T> {
    /// Create a new RMatrix dataset.
    pub fn new(data: Vec<RDataEntry<T>>) -> Result<Self, RError> {
        let (mut targets, records): (Vec<_>, Vec<_>) =
            data.into_iter().partition(|entry| entry.id.is_target());
        if records.is_empty() {
            return Err(RError::NoRecordEntryError);
        }
        match targets.len() {
            0 => Err(RError::NoTargetEntryError),
            1 => {
                let target = targets.pop().unwrap();
                let mean = math::mean(&target.data);
                let standard_deviation = math::standard_deviation(&target.data);
                let prior = math::probability_positive(&target.data);
                Ok(Self {
                    records,
                    target,
                    mean,
                    standard_deviation,
                    prior,
                })
            }
            targets => Err(RError::MultipleTargetEntriesError(targets)),
        }
    }
}

#[derive(Debug, Getters)]
#[getset(get = "pub")]
/// An entry in the RMatrix dataset.
pub struct RDataEntry<T> {
    /// The id of the entry defining the data.
    id: T,
    /// The data for the given entry.
    data: Vec<f64>,
}

impl<T: RMatrixId> RDataEntry<T> {
    /// Create a new RMatrix dataset entry.
    pub fn new(id: T, data: Vec<f64>) -> Self {
        Self { id, data }
    }
}
