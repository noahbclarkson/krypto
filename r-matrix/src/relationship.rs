use getset::Getters;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Getters)]
#[getset(get = "pub")]
/// A struct that represents a relationship between the target and record entries.
pub struct Relationship {
    /// The mean of the relationship values.
    mean: f64,
    /// The variance of the relationship values.
    variance: f64,
    /// The depth of the relationship.
    pub(crate) depth: usize,
}

impl Relationship {
    pub(crate) fn new(values: Vec<f64>, depth: usize) -> Self {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / values.len() as f64;
        Self {
            mean,
            variance,
            depth,
        }
    }
}
