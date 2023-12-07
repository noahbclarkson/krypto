use cmaes::{DVector, ObjectiveFunction};
use derive_builder::Builder;
use getset::Getters;

use crate::dataset::Dataset;

use super::matrix::RMatrix;

#[derive(Debug, Builder, Getters, Clone)]
#[getset(get = "pub")]
pub struct RMatrixCMAESSettings {
    #[builder(default = "CMAESOptimize::Accuracy")]
    optimize: CMAESOptimize,
}

#[derive(Debug, Clone)]
pub enum CMAESOptimize {
    Accuracy,
    Error,
    Cash,
}

pub struct RMatrixObjectiveFunction {
    r_matrix: Box<RMatrix>,
    dataset: Box<Dataset>,
    settings: RMatrixCMAESSettings,
}

impl RMatrixObjectiveFunction {
    pub fn new(r_matrix: RMatrix, dataset: Dataset, settings: RMatrixCMAESSettings) -> Self {
        Self {
            r_matrix: Box::new(r_matrix),
            dataset: Box::new(dataset),
            settings,
        }
    }
}

impl ObjectiveFunction for RMatrixObjectiveFunction {
    fn evaluate(&mut self, x: &DVector<f64>) -> f64 {
        let mut r_matrix = self.r_matrix.clone();
        r_matrix.set_weights(x.as_slice().to_vec());
        let test_data = r_matrix.test(&self.dataset);
        match self.settings.optimize {
            CMAESOptimize::Accuracy => test_data.get_accuracy(),
            CMAESOptimize::Error => test_data.get_mse(),
            CMAESOptimize::Cash => *test_data.cash(),
        }
    }
}
