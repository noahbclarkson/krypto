use getset::Getters;
use serde::{Deserialize, Serialize};

use crate::math::{standard_deviation, mean};

#[derive(Debug, Serialize, Deserialize, Getters, Clone)]
#[getset(get = "pub")]
/// A struct that represents a relationship between the target and record entries.
pub struct Relationship {
    /// The mean of the relationship values.
    mean: f64,
    /// The standard deviation of the relationship values
    standard_deviation: f64,
    /// The depth of the relationship.
    pub(crate) depth: usize,
}

impl Relationship {
    pub(crate) fn new(values: Vec<f64>, depth: usize) -> Self {
        Self {
            mean: mean(&values),
            standard_deviation: standard_deviation(&values),
            depth,
        }
    }
}

#[derive(Debug, Clone, Serialize, Getters, Deserialize)]
#[getset(get = "pub")]
pub struct RelationshipEntry {
    relationships: Box<[Relationship]>,
}

impl RelationshipEntry {
    pub fn new(relationships: Vec<Relationship>) -> Self {
        Self {
            relationships: relationships.into_boxed_slice(),
        }
    }
}
