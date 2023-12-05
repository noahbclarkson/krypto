use getset::Getters;

use crate::{
    dataset::{Dataset, Features},
    error::MatrixError,
};

pub trait Matrix {
    /// Train the matrix.
    fn train(&mut self, dataset: &Dataset) -> Result<(), Box<dyn MatrixError>>;

    /// Predict the label given the last x features.
    fn predict(
        &self,
        features: &[Features],
        forward_depth: usize,
        label_index: usize,
    ) -> Result<f64, Box<dyn MatrixError>>;

    /// Test the matrix.
    fn test(&self, dataset: &Dataset) -> Result<Box<dyn TestResult>, Box<dyn MatrixError>>;
}

pub trait TestResult {
    fn accuracy(&self) -> f64;
    fn correct(&self) -> &usize;
    fn incorrect(&self) -> &usize;
    fn total(&self) -> usize;
    fn mean_squared_error(&self) -> f64;
}
