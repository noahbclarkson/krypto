use std::fmt::{Display, Formatter};

use getset::{Getters, MutGetters, Setters};
use serde::{Deserialize, Serialize};

use crate::normalization_function::NormalizationFunctionType;

#[derive(Debug, Clone, Getters, Setters, Serialize, Deserialize)]
#[getset(get = "pub")]
pub struct RMatrixRelationship {
    feature_index: usize,
    label_index: usize,
    strength: f64,
    results: Vec<f64>,
    depth: usize,
    #[getset(set = "pub")]
    weight: f64,
}

impl RMatrixRelationship {
    pub fn new(feature_index: usize, label_index: usize, depth: usize) -> Self {
        Self {
            feature_index,
            label_index,
            strength: 0.0,
            results: Vec::new(),
            depth,
            weight: 1.0,
        }
    }

    pub fn add_result(&mut self, result: f64) {
        self.results.push(result);
    }

    pub fn compute_strength(&mut self, function: &NormalizationFunctionType) {
        let score: f64 = self.results.iter().sum();
        self.strength = function.get_function()(score / self.results.len() as f64);
    }
}

#[derive(Debug, Clone, Getters, MutGetters, Serialize, Deserialize)]
#[getset(get = "pub")]
pub struct RMatrixRelationshipMatrix {
    #[getset(get_mut = "pub")]
    relationships: Vec<RMatrixRelationship>,
    labels: usize,
    depth: usize,
}

impl RMatrixRelationshipMatrix {
    pub fn new(features: usize, labels: usize, depth: usize) -> Self {
        let mut relationships = Vec::with_capacity(features * labels * depth);
        for feature_index in 0..features {
            for label_index in 0..labels {
                for d in 1..=depth {
                    relationships.push(RMatrixRelationship::new(feature_index, label_index, d));
                }
            }
        }
        Self {
            relationships,
            labels,
            depth,
        }
    }

    pub fn get_relationship(
        &self,
        feature_index: usize,
        label_index: usize,
        depth: usize,
    ) -> Option<&RMatrixRelationship> {
        let index = feature_index * (self.labels * self.depth) + label_index * self.depth + depth;
        self.relationships.get(index)
    }

    pub fn get_relationship_mut(
        &mut self,
        feature_index: usize,
        label_index: usize,
        depth: usize,
    ) -> Option<&mut RMatrixRelationship> {
        let index = feature_index * (self.labels * self.depth) + label_index * self.depth + depth;
        self.relationships.get_mut(index)
    }

    pub fn compute_strengths(&mut self, function: &NormalizationFunctionType) {
        for relationship in self.relationships.iter_mut() {
            relationship.compute_strength(function);
        }
    }
}

impl Default for RMatrixRelationshipMatrix {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

impl Display for RMatrixRelationshipMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Relationship Matrix")?;
        writeln!(f, "===================")?;
        writeln!(f, "Depth: {}", self.depth())?;
        writeln!(f, "Relationships: {}", self.relationships.len())?;
        writeln!(f, "===================")?;
        writeln!(f, "Relationships")?;
        writeln!(f, "===================")?;
        for relationship in self.relationships.iter() {
            writeln!(f, "{}", relationship)?;
        }
        writeln!(f, "===================")?;
        Ok(())
    }
}

impl Display for RMatrixRelationship {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Feature Index: {}", self.feature_index())?;
        writeln!(f, "Label Index: {}", self.label_index())?;
        writeln!(f, "Strength: {}", self.strength())?;
        writeln!(f, "Results: {:?}", self.results())?;
        writeln!(f, "Depth: {}", self.depth())?;
        writeln!(f, "Weight: {}", self.weight())?;
        Ok(())
    }
}
