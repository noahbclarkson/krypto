use cmaes::{CMAESOptions, DVector, Mode, PlotOptions};
use derive_builder::Builder;
use getset::{Getters, Setters};
use plotters::prelude::*;

use crate::{
    dataset::{DataPoint, Dataset, Features},
    error::RMatrixError,
    math::max_index,
    normalization_function::NormalizationFunctionType,
    Labels,
};

use super::{
    cmaes::{CMAESOptimize, RMatrixCMAESSettings, RMatrixObjectiveFunction},
    relationship::{RMatrixRelationship, RMatrixRelationshipMatrix},
    test_data::TestData,
};

#[derive(Debug, Builder, Getters, Clone, Setters)]
#[getset(get = "pub", set = "pub")]
pub struct RMatrix {
    #[builder(default = "1")]
    depth: usize,
    #[builder(default = "NormalizationFunctionType::default()")]
    function: NormalizationFunctionType,
    #[builder(default = "RMatrixRelationshipMatrix::default()")]
    #[builder(setter(skip))]
    relationships: RMatrixRelationshipMatrix,
    #[builder(default = "1")]
    max_forward_depth: usize,
    #[builder(default = "1")]
    #[builder(setter(skip))]
    labels_len: usize,
    #[builder(default = "1")]
    #[builder(setter(skip))]
    features_len: usize,
    #[builder(default = "Vec::new()")]
    #[builder(setter(skip))]
    weights: Vec<f64>,
}

impl RMatrix {
    pub fn train(&mut self, dataset: &Dataset) -> Result<(), RMatrixError> {
        self.initialize_relationships_checked(dataset)?;
        self.process_dataset_for_relationships(dataset)?;
        Ok(())
    }

    fn initialize_relationships_checked(&mut self, dataset: &Dataset) -> Result<(), RMatrixError> {
        if dataset.is_empty() {
            return Err(RMatrixError::InvalidDatasetSize);
        }
        self.initialize_relationships(dataset);
        Ok(())
    }

    fn initialize_relationships(&mut self, dataset: &Dataset) {
        self.labels_len = dataset.labels_len();
        self.features_len = dataset.features_len();
        self.relationships =
            RMatrixRelationshipMatrix::new(self.features_len, self.labels_len, self.depth);
    }

    fn process_dataset_for_relationships(&mut self, dataset: &Dataset) -> Result<(), RMatrixError> {
        for window in dataset.windowed_iter(self.depth + 1) {
            self.update_relationships_checked(window)?;
        }
        self.relationships.compute_strengths(&self.function);
        Ok(())
    }

    fn update_relationships_checked(&mut self, window: &[DataPoint]) -> Result<(), RMatrixError> {
        if window.len() < self.depth + 1 {
            return Err(RMatrixError::InvalidDatasetSize);
        }
        self.update_relationships(window)
    }

    fn update_relationships(&mut self, window: &[DataPoint]) -> Result<(), RMatrixError> {
        if window.len() < self.depth + 1 {
            return Err(RMatrixError::InvalidDatasetSize);
        }

        let f = self.function.get_function();
        for depth in 0..self.depth {
            self.update_relationships_at_depth(window, depth, &f)?;
        }
        Ok(())
    }

    fn update_relationships_at_depth(
        &mut self,
        window: &[DataPoint],
        depth: usize,
        normalization_function: &dyn Fn(f64) -> f64,
    ) -> Result<(), RMatrixError> {
        let labels = match window.get(self.depth) {
            Some(data_point) => data_point.labels(),
            None => return Err(RMatrixError::InvalidDataPointIndex(self.depth)),
        };
        let features = match window.get(self.depth - depth - 1) {
            Some(data_point) => data_point.features(),
            None => return Err(RMatrixError::InvalidDataPointIndex(self.depth - depth - 1)),
        };

        for (label_index, label) in labels.iter().enumerate() {
            for feature_index in 0..self.features_len {
                let relationship = self.get_relationship_mut(feature_index, label_index, depth)?;
                let result = normalization_function(features[feature_index] * label);
                relationship.add_result(result);
            }
        }
        Ok(())
    }

    pub fn raw_predict(
        &self,
        features: &[Features],
        label_index: usize,
    ) -> Result<Vec<f64>, RMatrixError> {
        let f = self.function.get_function();
        let mut results: Vec<Vec<f64>> = vec![Vec::new(); self.max_forward_depth];
        let weights = if self.weights.is_empty() {
            vec![1.0; self.depth]
        } else {
            self.weights.clone()
        };
        for (backward_depth, feature_array) in features.iter().rev().enumerate() {
            for (f_index, feature_value) in feature_array.iter().enumerate() {
                for real_depth in backward_depth..self.depth {
                    let forward_depth = self.depth - real_depth;
                    if forward_depth > self.max_forward_depth {
                        continue;
                    }
                    let relationship = self.get_relationship(f_index, label_index, real_depth)?;
                    let result =
                        f(feature_value * relationship.strength()) * weights[forward_depth - 1];
                    results[forward_depth - 1].push(result);
                }
            }
        }
        let final_results: Vec<f64> = results
            .iter()
            .map(|result| result.iter().sum::<f64>() / result.len() as f64)
            .collect();
        Ok(final_results)
    }

    pub fn test(&self, dataset: &Dataset) -> TestData {
        let mut test_data = TestData::default();
        for window in dataset.windowed_iter(self.depth * 2) {
            let (features, labels) = self.split_window(window).unwrap();
            let predictions = self.generate_predictions(&features).unwrap();
            let max_index = max_index(&predictions);
            let next_periods = labels[max_index].data();
            let prediction = predictions[max_index];
            let total_pc = next_periods.iter().sum::<f64>();
            test_data.add_error(prediction, total_pc);
            test_data.add_cash(test_data.cash() * total_pc * prediction.signum());
        }
        test_data
    }

    fn split_window(
        &self,
        window: &[DataPoint],
    ) -> Result<(Vec<Features>, Vec<Labels>), RMatrixError> {
        let features = window[0..self.depth]
            .iter()
            .map(|d| d.features().clone())
            .collect();
        let labels = window[self.depth..self.depth * 2]
            .iter()
            .map(|d| d.labels().clone())
            .collect();
        Ok((features, labels))
    }

    fn generate_predictions(&self, features: &[Features]) -> Result<Vec<f64>, RMatrixError> {
        let mut predictions = Vec::with_capacity(self.labels_len);
        for label_index in 0..self.labels_len {
            let prediction = self.raw_predict(features, label_index)?;
            predictions.push(prediction.iter().sum());
        }
        Ok(predictions)
    }

    pub fn optimize(&mut self, dataset: &Dataset, settings: RMatrixCMAESSettings) {
        let obj_function =
            RMatrixObjectiveFunction::new(self.clone(), dataset.clone(), settings.clone());
        let start = DVector::from_vec(vec![1.0; self.depth]);
        let mode = match settings.optimize() {
            CMAESOptimize::Accuracy => Mode::Maximize,
            CMAESOptimize::Error => Mode::Minimize,
            CMAESOptimize::Cash => Mode::Maximize,
        };
        let mut cmaes = CMAESOptions::new(start, 0.1)
            .mode(mode)
            // .population_size(50)
            .initial_step_size(0.05)
            // .tol_fun_hist(1e-14)
            .enable_printing(100)
            .enable_plot(PlotOptions::new(3, false))
            .build(obj_function)
            .unwrap();
        let results = cmaes.run();
        cmaes
            .get_plot()
            .unwrap()
            .save_to_file("plot.png", true)
            .unwrap();
        match results.overall_best {
            Some(best) => {
                self.weights = best.point.as_slice().to_vec();
            }
            None => {
                panic!("CMAES failed to find a solution.");
            }
        }
    }

    pub fn plot_cash(&self, dataset: &Dataset) -> Result<(), RMatrixError> {
        let t_data = self.test(dataset);
        let cashes = t_data.cash_history();

        let root = BitMapBackend::new("cash.png", (1280, 720)).into_drawing_area();
        root.fill(&WHITE).unwrap();
        let max_cash = cashes
            .iter()
            .filter_map(|c| if *c > 0.0 { Some(*c) } else { None })
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();

        let mut chart = ChartBuilder::on(&root)
            .caption("Cash", ("sans-serif", 50).into_font())
            .margin(5)
            .x_label_area_size(30)
            .y_label_area_size(40)
            .build_cartesian_2d(0..cashes.len(), (1.0..max_cash * 1.1).log_scale())
            .unwrap();

        chart
            .configure_mesh()
            .x_desc("Time")
            .y_desc("Cash (log scale)")
            .axis_desc_style(("sans-serif", 15).into_font())
            .draw()
            .unwrap();

        chart
            .draw_series(LineSeries::new(
                (0..cashes.len()).map(|i| (i, cashes[i].max(1.0))), // Avoid log(0) by ensuring cash is at least 1.0
                &RED,
            ))
            .unwrap()
            .label("Cash Over Time")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED));

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .border_style(BLACK)
            .draw()
            .unwrap();

        Ok(())
    }

    fn get_relationship_mut(
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

    fn get_relationship(
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
}
