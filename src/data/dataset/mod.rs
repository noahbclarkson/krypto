use std::collections::HashMap;

use symbol_data::RawSymbolData;

use crate::util::matrix_utils::normalize_by_columns;

pub mod interval_data;
pub mod symbol_data;
pub mod overall_dataset;

fn get_normalized_predictors(records: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    normalize_by_columns(records)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| if v.is_nan() { 0.0 } else { v })
                .collect()
        })
        .collect::<Vec<Vec<f64>>>()
}

pub fn get_records(map: &HashMap<String, RawSymbolData>) -> Vec<Vec<f64>> {
    let mut records = Vec::new();
    for i in 0..map.values().next().unwrap().len() {
        let mut record = Vec::new();
        for symbol_data in map.values() {
            let technicals = symbol_data.get_technicals();
            record.extend(technicals[i].as_array().to_vec());
        }
        records.push(record);
    }
    records
}
