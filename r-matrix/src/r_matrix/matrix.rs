use derive_builder::Builder;
use getset::Getters;

use crate::{
    dataset::{DataPoint, Dataset, Features, Labels},
    error::{MatrixError, RMatrixError},
    matrix::{Matrix, TestResult},
    normalization_function::NormalizationFunctionType,
    r_matrix::r_matrix_relationship::RMatrixRelationship,
};

use super::{
    r_matrix_relationship::RMatrixRelationshipMatrix, r_matrix_test_result::RMatrixTestResult,
};

#[derive(Debug, Builder, Getters)]
#[getset(get = "pub")]
pub struct RMatrix {
    #[builder(default = "1")]
    depth: usize,
    #[builder(default = "NormalizationFunctionType::default()")]
    function: NormalizationFunctionType,
    dataset: Box<Dataset>,
    #[builder(default = "0.0")]
    minimum_strength: f64,
    #[builder(default = "RMatrixRelationshipMatrix::default()")]
    #[builder(setter(skip))]
    relationships: RMatrixRelationshipMatrix,
    #[builder(default = "1")]
    max_forward_depth: usize,
}

impl Matrix for RMatrix {
    fn train(&mut self, dataset: &Dataset) -> Result<(), Box<dyn MatrixError>> {
        self.relationships = RMatrixRelationshipMatrix::new(
            dataset.feature_names().len(),
            dataset.label_names().len(),
            self.depth,
        );
        let iter = dataset.windowed_iter(self.depth + 1);
        for window in iter {
            self.update_relationships(window)
                .map_err(|e| Box::new(e) as Box<dyn MatrixError>)?;
        }
        self.relationships.compute_strengths(&self.function.clone());
        Ok(())
    }

    fn predict(
        &self,
        features: &[Features],
        forward_depth: usize,
        label_index: usize,
    ) -> Result<f64, Box<dyn MatrixError>> {
        if forward_depth > self.max_forward_depth {
            return Err(Box::new(RMatrixError::ForwardDepthTooLarge(
                forward_depth,
                self.max_forward_depth,
            )));
        }
        if features.len() < self.depth {
            return Err(Box::new(RMatrixError::WrongNumberOfFeatures(
                features.len(),
                self.depth,
            )));
        }
        for (depth, feauture_array) in features.iter().rev().enumerate() {
            for (feature_index, feature_value) in feauture_array.iter().enumerate() {
                for d in depth..self.depth {
                    let relationship = self.get_relationship(feature_index, label_index, d).map_err(|e| Box::new(e) as Box<dyn MatrixError>)?;
                    let forward_index = self.depth - d;
                    
                }
            }
        }
        Ok(())
    }

    fn test(&self, dataset: &Dataset) -> Result<Box<dyn TestResult>, Box<dyn MatrixError>> {
        let mut test = RMatrixTestResult::default();
        let iter = dataset.windowed_iter(self.depth + 1);
        for window in iter {
            let features = get_features_from_window(window, 0)
                .map_err(|e| Box::new(e) as Box<dyn MatrixError>)?;
        }
        Ok(Box::new(test))
    }
}

impl RMatrix {
    fn update_relationships(&mut self, window: &[DataPoint]) -> Result<(), RMatrixError> {
        let function = self.function.get_function();
        for d in 0..self.depth {
            let labels = get_labels_from_window(window, self.depth + 1)?;
            let feature = get_features_from_window(window, self.depth - d)?;
            for (feature_index, feature_value) in feature.iter().enumerate() {
                for (label_index, label_value) in labels.iter().enumerate() {
                    let relationship = self.get_relationship_mut(feature_index, label_index, d)?;
                    let result = function(feature_value * label_value);
                    relationship.add_result(result);
                }
            }
        }

        Ok(())
    }

    pub fn get_relationship(
        &self,
        feature_index: usize,
        label_index: usize,
        depth: usize,
    ) -> Result<&RMatrixRelationship, RMatrixError> {
        self.relationships
            .get_relationship(feature_index, label_index, depth)
            .ok_or(RMatrixError::CantFindRelationship(
                feature_index,
                label_index,
                depth,
            ))
    }

    pub fn get_relationship_mut(
        &mut self,
        feature_index: usize,
        label_index: usize,
        depth: usize,
    ) -> Result<&mut RMatrixRelationship, RMatrixError> {
        self.relationships
            .get_relationship_mut(feature_index, label_index, depth)
            .ok_or(RMatrixError::CantFindRelationship(
                feature_index,
                label_index,
                depth,
            ))
    }
}

fn get_features_from_window(window: &[DataPoint], index: usize) -> Result<&Features, RMatrixError> {
    Ok(window
        .get(index)
        .ok_or(RMatrixError::CantIndexFeatures(index))?
        .features())
}

fn get_labels_from_window(window: &[DataPoint], index: usize) -> Result<&Labels, RMatrixError> {
    Ok(window
        .get(index)
        .ok_or(RMatrixError::CantIndexFeatures(index))?
        .labels())
}
