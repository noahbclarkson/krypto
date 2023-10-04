use errors::RError;
use getset::Getters;
use math::NormalizationFunctionType;
use relationship::Relationship;
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
    relationships: Vec<Relationship>,
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
            relationships: Vec::new(),
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
        self.relationships = futures::future::join_all(tasks)
            .await
            .into_iter()
            .flatten()
            .collect();
    }

    async fn compute(&self, record: &RDataEntry<T>, target: &RDataEntry<T>) -> Vec<Relationship> {
        let max_depth = self.max_depth;
        let length = record.data.len();
        let mut results = vec![Vec::new(); max_depth];
        for i in max_depth..length.saturating_sub(1) {
            let target_value = target.data.get(i + 1).unwrap_or(&0.0);
            for depth in 0..max_depth {
                let record_value = record.data.get(i - depth).unwrap_or(&0.0);
                let result = self.function.get_function()(*record_value * *target_value).tanh();
                results[depth].push(result);
            }
        }
        results
            .into_iter()
            .enumerate()
            .map(|(depth, values)| Relationship::new(values, depth + 1))
            .collect()
    }

    #[inline]
    /// A variant of the predict function that checks that all data is valid before predicting.
    pub async fn predict_stable(&self, data: &RData<T>, index: usize) -> Result<f64, RError> {
        if data.records.len() != self.relationships.len() {
            return Err(RError::RelationshipRecordCountMismatchError);
        }
        let mut score = 0.0;
        for (i, entry) in data.records.iter().enumerate() {
            let relationship = &self.relationships[i];
            if let Some(record) = entry.data.get(index.saturating_sub(relationship.depth)) {
                score += self.calculate_score(relationship, *record);
            } else {
                return Err(RError::RecordIndexOutOfBoundsError);
            }
        }
        Ok(score)
    }

    #[inline(always)]
    /// Calculate the score for the given data at the given index.
    pub async fn predict(&self, data: &RData<T>, index: usize) -> f64 {
        data.records
            .iter()
            .enumerate()
            .map(|(i, entry)| self.get_and_calculate_score(entry, i, index))
            .sum::<f64>()
    }

    #[inline(always)]
    fn get_and_calculate_score(&self, entry: &RDataEntry<T>, target: usize, index: usize) -> f64 {
        let relationship = &self.relationships[target];
        let record = entry.data[index - relationship.depth];
        self.calculate_score(relationship, record)
    }

    #[inline(always)]
    fn calculate_score(&self, relationship: &Relationship, record: f64) -> f64 {
        let input = record * relationship.mean();
        let score = self.function.get_function()(input).tanh();
        score * relationship.variance()
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
            1 => Ok(Self {
                records,
                target: targets.pop().unwrap(),
            }),
            _ => Err(RError::MultipleTargetEntriesError),
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
