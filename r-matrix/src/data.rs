use getset::Getters;

use crate::{errors::RError, math};

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
    /// Each record entry contains all the data for a given RMatrixId
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

    /// Transpose the records.
    pub fn transpose_records(&self) -> Vec<Vec<f64>> {
        let mut records = Vec::with_capacity(self.records[0].data.len());
        for i in 0..self.records[0].data.len() {
            let mut record = Vec::with_capacity(self.records.len());
            for entry in self.records.iter() {
                record.push(entry.data[i]);
            }
            records.push(record);
        }
        records
    }

    /// Normalize the data
    /// Each record is normalized as how many standard deviations it is from the mean.
    /// The target is not normalized.
    pub fn normalize(&mut self) {
        for entry in self.records.iter_mut() {
            entry.normalize();
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

    fn normalize(&mut self) {
        let mean = math::mean(&self.data);
        let standard_deviation = math::standard_deviation(&self.data);
        for value in self.data.iter_mut() {
            *value = (*value - mean) / standard_deviation;
        }
    }
}
