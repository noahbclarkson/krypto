use std::fmt::{Display, Formatter};

use cmaes::{DVector, ObjectiveFunction};
use derive_builder::Builder;
use getset::Getters;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{dataset::Dataset, return_calculator::ReturnCalculator};

use super::matrix::RMatrix;

#[derive(Debug, Builder, Getters, Clone)]
#[getset(get = "pub")]
pub struct RMatrixCMAESSettings {
    #[builder(default = "CMAESOptimize::Accuracy")]
    optimize: CMAESOptimize,
    #[builder(default = "false")]
    with_individuals: bool,
    interval: usize,
}

#[derive(Debug, Clone)]
pub enum CMAESOptimize {
    Accuracy,
    Error,
    Cash,
}

impl Display for CMAESOptimize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CMAESOptimize::Accuracy => write!(f, "Accuracy"),
            CMAESOptimize::Error => write!(f, "Error"),
            CMAESOptimize::Cash => write!(f, "Cash"),
        }
    }
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

    fn calc_with_individuals(&self, r_matrix: &mut RMatrix, x: &DVector<f64>) -> f64 {
        // Set the first r_matrix.depth in x to the weights as above
        let depth = *r_matrix.depth();
        r_matrix.set_weights(x.as_slice().to_vec()[..depth].to_vec());
        // Now we go through every relationship and set the weight to the value in x
        for (i, relationship) in r_matrix
            .relationships_mut()
            .relationships_mut()
            .iter_mut()
            .enumerate()
        {
            relationship.set_weight(x[i + depth]);
        }
        let test_data = r_matrix.test(&self.dataset);
        match self.settings.optimize {
            CMAESOptimize::Accuracy => test_data.get_accuracy(),
            CMAESOptimize::Error => test_data.get_mse(),
            CMAESOptimize::Cash => *test_data.cash(),
        }
    }
}

impl ObjectiveFunction for RMatrixObjectiveFunction {
    fn evaluate(&mut self, x: &DVector<f64>) -> f64 {
        let mut r_matrix = self.r_matrix.clone();
        if self.settings.with_individuals {
            return self.calc_with_individuals(&mut r_matrix, x);
        }
        r_matrix.set_weights(x.as_slice().to_vec());
        let test_data = r_matrix.test(&self.dataset);
        match self.settings.optimize {
            CMAESOptimize::Accuracy => test_data.get_accuracy(),
            CMAESOptimize::Error => test_data.get_mse(),
            CMAESOptimize::Cash => {
                let calculator = ReturnCalculator::new(
                    self.settings.interval,
                    test_data.cash_history().clone(),
                    *test_data.hold_periods(),
                );
                calculator.average_daily_return()
            }
        }
    }
}

impl Serialize for CMAESOptimize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CMAESOptimize::Accuracy => serializer.serialize_str("accuracy"),
            CMAESOptimize::Error => serializer.serialize_str("error"),
            CMAESOptimize::Cash => serializer.serialize_str("cash"),
        }
    }
}

impl<'de> Deserialize<'de> for CMAESOptimize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "accuracy" => Ok(CMAESOptimize::Accuracy),
            "error" => Ok(CMAESOptimize::Error),
            "cash" => Ok(CMAESOptimize::Cash),
            _ => panic!("Invalid CMAESOptimize value"),
        }
    }
}
