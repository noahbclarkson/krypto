use getset::Getters;

use crate::matrix::TestResult;

#[derive(Debug, Getters, Default)]
#[getset(get = "pub")]
pub struct RMatrixTestResult {
    pub(crate) correct: usize,
    pub(crate) incorrect: usize,
    #[getset(skip)]
    pub(crate) prediction_real_map: Vec<(f64, f64)>,
}

impl TestResult for RMatrixTestResult {
    fn accuracy(&self) -> f64 {
        self.correct as f64 / self.total() as f64
    }

    fn correct(&self) -> &usize {
        &self.correct
    }

    fn incorrect(&self) -> &usize {
        &self.incorrect
    }

    fn total(&self) -> usize {
        self.correct + self.incorrect
    }

    fn mean_squared_error(&self) -> f64 {
        let mut sum = 0.0;
        for (prediction, real) in self.prediction_real_map.iter() {
            sum += (prediction - real).powi(2);
        }
        sum / self.prediction_real_map.len() as f64
    }
}
