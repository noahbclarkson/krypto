use getset::{Getters, MutGetters};

use crate::error::DatasetError;
use std::cmp::Ordering;

#[derive(Clone, Debug, Getters)]
#[getset(get = "pub")]
pub struct DataPoint {
    time: usize,
    features: Features,
    labels: Labels,
}

impl DataPoint {
    pub fn new(time: usize, features: Vec<f64>, labels: Vec<f64>) -> Self {
        Self {
            time,
            features: Features::new(features),
            labels: Labels::new(labels),
        }
    }
}

impl PartialEq for DataPoint {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}

impl Eq for DataPoint {}

impl PartialOrd for DataPoint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DataPoint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.cmp(&other.time)
    }
}

#[derive(Clone, Debug, Getters, MutGetters)]
pub struct Dataset {
    #[getset(get = "pub")]
    data: Vec<DataPoint>,
    #[getset(get = "pub", get_mut = "pub")]
    feature_names: Vec<String>,
    #[getset(get = "pub", get_mut = "pub")]
    label_names: Vec<String>,
}

impl Dataset {
    pub fn builder() -> DatasetBuilder {
        DatasetBuilder::default()
    }

    pub fn index(&self, index: usize) -> Option<&DataPoint> {
        self.data.get(index)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn sort_by_time(&mut self) {
        self.data.sort();
    }

    pub fn feature_name_at_index(&self, index: usize) -> Option<&String> {
        self.feature_names.get(index)
    }

    pub fn label_name_at_index(&self, index: usize) -> Option<&String> {
        self.label_names.get(index)
    }

    pub fn iter(&self) -> std::slice::Iter<DataPoint> {
        self.data.iter()
    }

    pub fn windowed_iter(&self, window_size: usize) -> DatasetWindowIterator {
        DatasetWindowIterator {
            dataset: self,
            window_size,
            start_index: 0,
        }
    }

    pub fn split(&self, ratio: f64) -> (Self, Self) {
        let split_index = (self.data.len() as f64 * ratio) as usize;
        let (left, right) = self.data.split_at(split_index);

        (
            Dataset {
                data: left.to_vec(),
                feature_names: self.feature_names.clone(),
                label_names: self.label_names.clone(),
            },
            Dataset {
                data: right.to_vec(),
                feature_names: self.feature_names.clone(),
                label_names: self.label_names.clone(),
            },
        )
    }
}

#[derive(Default)]
pub struct DatasetBuilder {
    data: Vec<DataPoint>,
    feature_names: Option<Vec<String>>,
    label_names: Option<Vec<String>>,
}

impl DatasetBuilder {
    pub fn add_data_point(
        &mut self,
        time: usize,
        features: Vec<f64>,
        labels: Vec<f64>,
    ) -> &mut Self {
        self.data.push(DataPoint::new(time, features, labels));
        self
    }

    pub fn set_feature_names(&mut self, names: Vec<String>) -> &mut Self {
        self.feature_names = Some(names);
        self
    }

    pub fn set_label_names(&mut self, names: Vec<String>) -> &mut Self {
        self.label_names = Some(names);
        self
    }

    pub fn with_data(mut self, data: Vec<DataPoint>) -> Self {
        self.data = data;
        self
    }

    pub fn build(self) -> Result<Dataset, DatasetError> {
        let feature_count = self.data.first().map_or(0, |dp| dp.features.len());
        let label_count = self.data.first().map_or(0, |dp| dp.labels.len());

        let feature_names = self.feature_names.unwrap_or_else(|| {
            (1..=feature_count)
                .map(|i| format!("feature_{}", i))
                .collect()
        });
        let label_names = self
            .label_names
            .unwrap_or_else(|| (1..=label_count).map(|i| format!("label_{}", i)).collect());

        Ok(Dataset {
            data: self.data,
            feature_names,
            label_names,
        })
    }
}

pub struct DatasetWindowIterator<'a> {
    dataset: &'a Dataset,
    window_size: usize,
    start_index: usize,
}

impl<'a> Iterator for DatasetWindowIterator<'a> {
    type Item = &'a [DataPoint];

    fn next(&mut self) -> Option<Self::Item> {
        if self.start_index + self.window_size <= self.dataset.data.len() {
            let window = &self.dataset.data[self.start_index..self.start_index + self.window_size];

            self.start_index += 1; // Move to the next window
            Some(window)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Getters)]
#[getset(get = "pub")]
pub struct Features {
    data: Vec<f64>,
}

impl Features {
    pub fn new(data: Vec<f64>) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<f64> {
        self.data.iter()
    }
}

#[derive(Clone, Debug, Getters)]
#[getset(get = "pub")]
pub struct Labels {
    data: Vec<f64>,
}

impl Labels {
    pub fn new(data: Vec<f64>) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<f64> {
        self.data.iter()
    }
}
