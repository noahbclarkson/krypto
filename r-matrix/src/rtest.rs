use derive_builder::Builder;
use getset::Getters;

use crate::{
    data::{RData, RMatrixId},
    matricies::RMatrix,
};

#[derive(Debug, Builder, Default, Clone)]
pub struct RTestConfig {
    pub starting_cash: f64,
    pub margin: f64,
    pub starting_position: usize,
    #[builder(default = "0.0")]
    pub min_change: f64,
}

#[derive(Debug, Getters, Clone)]
#[getset(get = "pub")]
pub struct RTest<T> {
    cash: f64,
    cash_history: Vec<f64>,
    correct: usize,
    incorrect: usize,
    predictions: Vec<f64>,
    pub config: RTestConfig,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: RMatrixId> RTest<T> {
    pub fn new(matrix: Box<dyn RMatrix<T>>, rdata: &RData<T>, config: RTestConfig) -> Self {
        let mut cash = config.starting_cash;
        let mut correct = 0;
        let mut incorrect = 0;
        let mut cash_history = Vec::new();
        let mut predictions = Vec::new();
        for i in config.starting_position..rdata.target().data().len() {
            let prediction = matrix.predict(&rdata, i);
            predictions.push(prediction);
            let actual = rdata.target().data()[i];
            if prediction * actual > 0.0 && prediction.abs() > config.min_change {
                cash += cash * actual.abs() * config.margin;
                correct += 1;
            } else if prediction * actual < 0.0 && prediction.abs() > config.min_change {
                cash -= cash * actual.abs() * config.margin;
                incorrect += 1;
            }
            cash_history.push(cash);
        }
        Self {
            cash,
            correct,
            incorrect,
            cash_history,
            predictions,
            config,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn accuracy(&self) -> f64 {
        self.correct as f64 / (self.correct + self.incorrect) as f64
    }

    pub fn cash_string(&self) -> String {
        format!("${}", crate::math::format_number(self.cash))
    }

    pub fn retest(&mut self, rdata: &RData<T>)  {
        let mut cash = self.config.starting_cash;
        let mut correct = 0;
        let mut incorrect = 0;
        let mut cash_history = Vec::new();
        let mut p_index = 0;
        for i in self.config.starting_position..rdata.target().data().len() {
            let prediction = self.predictions[p_index];
            p_index += 1;
            let actual = rdata.target().data()[i];
            if prediction * actual > 0.0 && prediction.abs() > self.config.min_change {
                cash += cash * actual.abs() * self.config.margin;
                correct += 1;
            } else if prediction * actual < 0.0 && prediction.abs() > self.config.min_change {
                cash -= cash * actual.abs() * self.config.margin;
                incorrect += 1;
            }
            cash_history.push(cash);
        }
        self.cash = cash;
        self.correct = correct;
        self.incorrect = incorrect;
        self.cash_history = cash_history;
    }
}
