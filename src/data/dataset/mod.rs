use std::collections::HashMap;

use crate::{error::KryptoError, util::matrix_utils::normalize_by_columns};

use self::symbol_data::RawSymbolData;

pub mod cache; // Added cache module
pub mod interval_data;
pub mod overall_dataset;
pub mod symbol_data;

/// Normalizes a matrix (Vec<Vec<f64>>) by columns (features).
/// Each column will have a mean of 0 and a standard deviation of 1.
/// Handles NaN values by replacing them with 0.0 *after* normalization.
fn get_normalized_predictors(records: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    if records.is_empty() || records[0].is_empty() {
        return Vec::new(); // Return empty if input is empty
    }

    normalize_by_columns(records) // Assumes normalize_by_columns handles potential errors or NaNs during calculation
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| {
                    if v.is_nan() || v.is_infinite() {
                        // warn!("NaN or Inf detected during normalization, replacing with 0.0");
                        0.0
                    } else {
                        v
                    }
                })
                .collect()
        })
        .collect::<Vec<Vec<f64>>>()
}

/// Extracts technical indicator records from the symbol data map.
/// Creates a matrix where each row represents a time step and columns are concatenated
/// technical indicators for all symbols.
/// Returns error if data is inconsistent.
pub fn get_records(map: &HashMap<String, RawSymbolData>) -> Result<Vec<Vec<f64>>, KryptoError> {
    if map.is_empty() {
        return Ok(Vec::new());
    }

    // Find the expected length based on the first non-empty symbol data
    let expected_len = map
        .values()
        .find(|sd| !sd.is_empty())
        .map(|sd| sd.len())
        .ok_or_else(|| KryptoError::InsufficientData {
            got: 0,
            required: 1,
            context: "No non-empty symbol data found to determine record length".to_string(),
        })?;

    if expected_len == 0 {
        return Ok(Vec::new()); // All symbols have zero length data
    }

    let mut records = Vec::with_capacity(expected_len);
    for i in 0..expected_len {
        let mut record = Vec::new();
        for symbol_data in map.values() {
            // Check for length consistency
            if symbol_data.len() != expected_len {
                return Err(KryptoError::InvalidDatasetLengths(
                    expected_len,                       // Expected features/labels/candles length
                    symbol_data.get_technicals().len(), // Actual technicals length for this symbol
                    symbol_data.len(),                  // Overall length for this symbol
                ));
            }

            let technicals = symbol_data.get_technicals();
            // Ensure we don't index out of bounds (should be caught by length check above)
            if i < technicals.len() {
                record.extend(technicals[i].as_array());
            } else {
                // This case should ideally not happen if length checks pass
                return Err(KryptoError::InsufficientData {
                    got: technicals.len(),
                    required: expected_len,
                    context: format!(
                        "Inconsistent technicals length for symbol {} at index {}",
                        symbol_data.symbol(),
                        i
                    ),
                });
            }
        }
        records.push(record);
    }
    Ok(records)
}
