use std::collections::HashMap;

use crate::{
    algorithm::Ratio,
    candle::{Candle, TECHNICAL_COUNT},
    config::Config,
    ticker_data::TickerData,
};
use getset::Getters;

/// Represents a relationship between technical indicators from two sets of ticker data.
/// Each relationship includes a correlation value, depth, type of relationship, and the indices of target and prediction data.
#[derive(Debug, Clone, PartialEq, Getters)]
#[getset(get = "pub")]
pub struct Relationship {
    correlation: f64,
    depth: usize,
    r_type: usize,
    target_index: usize,
    predict_index: usize,
}

/// Computes a list of `Relationship` objects asynchronously.
///
/// This function is responsible for calculating how each set of ticker data (target)
/// might be influenced by every other set (predictor).
///
/// # Parameters
///
/// - `candles`: A slice containing the ticker data for multiple stocks.
/// - `config`: Configuration options, including the `depth` parameter.
///
/// # Returns
///
/// A boxed slice containing all calculated `Relationship` objects.
pub async fn compute_relationships(
    candles: &[TickerData],
    config: &Config,
    ratio: Ratio,
) -> Box<[Relationship]> {
    let mut relationships = Vec::new();
    for (target_index, target_candles) in candles.iter().enumerate() {
        let tasks: Vec<_> = candles
            .iter()
            .enumerate()
            .map(|(predict_index, predict_candles)| {
                compute_relationship(
                    target_index,
                    predict_index,
                    target_candles,
                    predict_candles,
                    *config.depth(),
                    ratio.clone(),
                )
            })
            .collect();
        let new_relationships: Vec<_> = futures::future::join_all(tasks).await;
        relationships.extend(new_relationships.into_iter().flatten());
    }
    relationships.into_boxed_slice()
}

/// Compute an individual `Relationship` object asynchronously.
///
/// # Parameters
///
/// - `target_index`: The index of the target ticker data in the `candles` array.
/// - `predict_index`: The index of the predicting ticker data in the `candles` array.
/// - `target_candles`: The ticker data for the target.
/// - `predict_candles`: The ticker data for the predictor.
/// - `depth`: Depth of the technical analysis.
///
/// # Returns
///
/// A vector of `Relationship` objects between the target and predictor with various depths and technical indicators.
async fn compute_relationship(
    target_index: usize,
    predict_index: usize,
    target_candles: &TickerData,
    predict_candles: &TickerData,
    depth: usize,
    ratio: Ratio,
) -> Vec<Relationship> {
    let mut results = vec![Vec::new(); TECHNICAL_COUNT * depth];
    let length = predict_candles.candles().len() as f64;
    let start = (depth + 1) + (length * ratio.start()) as usize;
    let end = (length * ratio.end()) as usize - 1;
    for i in start..end {
        populate_results(
            &mut results,
            target_candles.candles(),
            predict_candles.candles(),
            i,
            depth,
        );
    }
    let correlations: Vec<f64> = results
        .iter()
        .map(|v| v.iter().copied().sum::<f64>() / v.len() as f64)
        .collect();
    build_relationships(correlations, target_index, predict_index, depth)
}

/// Populate result vectors for relationship calculations.
///
/// # Parameters
///
/// - `results`: Mutable reference to a vector that will hold the correlation results.
/// - `target_candles`: Array of `Candle` objects for the target.
/// - `predict_candles`: Array of `Candle` objects for the predictor.
/// - `i`: The current index of prediction within the `predict_candles`.
/// - `depth`: Depth of the technical analysis.
fn populate_results(
    results: &mut [Vec<f64>],
    target_candles: &[Candle],
    predict_candles: &[Candle],
    i: usize,
    depth: usize,
) {
    let target = target_candles[i + 1].percentage_change();
    for d in 0..depth {
        for (j, technical) in predict_candles[i - d].technicals().iter().enumerate() {
            results[d * TECHNICAL_COUNT + j].push((technical * target).tanh());
        }
    }
}

/// Construct a vector of `Relationship` objects based on the computed correlations.
///
/// # Parameters
///
/// - `correlations`: A vector containing calculated correlations.
/// - `target_index`: Index of the target in the main `candles` array.
/// - `predict_index`: Index of the predictor in the main `candles` array.
/// - `depth`: Depth of the technical analysis.
///
/// # Returns
///
/// A vector of `Relationship` objects.
fn build_relationships(
    correlations: Vec<f64>,
    target_index: usize,
    predict_index: usize,
    depth: usize,
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    for d in 0..depth {
        for j in 0..TECHNICAL_COUNT {
            let correlation = correlations[d * TECHNICAL_COUNT + j];
            relationships.push(Relationship {
                correlation,
                depth: d + 1,
                r_type: j,
                target_index,
                predict_index,
            });
        }
    }
    relationships
}

/// Predicts the best target based on computed relationships.
///
/// # Parameters
///
/// - `relationships`: A slice of `Relationship` objects.
/// - `current_position`: The current index in the `candles` array for real-time prediction.
/// - `candles`: A slice containing the ticker data for multiple stocks.
///
/// # Returns
///
/// A tuple containing the index of the best target and its score.
#[inline(always)]
pub fn predict(
    relationships: &[Relationship],
    current_position: usize,
    candles: &[TickerData],
) -> Prediction {
    let mut scores = vec![0.0; candles.len()];
    for relationship in relationships {
        for d in 0..relationship.depth {
            let predict = candles[relationship.predict_index].candles()[current_position - d]
                .technicals()[relationship.r_type];
            scores[relationship.target_index] += (predict * relationship.correlation).tanh();
        }
    }
    let (max_index, max) = scores
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .unwrap();
    Prediction::new(max_index, *max)
}

#[derive(Debug, Getters, Clone)]
#[getset(get = "pub")]
pub struct Prediction {
    target_index: usize,
    score: f64,
}

impl Prediction {
    pub fn new(target_index: usize, score: f64) -> Self {
        Self {
            target_index,
            score,
        }
    }
}

/// Predicts the best target based on computed relationships.
///
/// # Parameters
///
/// - `relationships`: A slice of `Relationship` objects.
/// - `current_position`: The current index in the `candles` array for real-time prediction.
/// - `candles`: A slice containing the ticker data for multiple stocks.
///
/// # Returns
///
/// A vector of tuples containing the index of the best target and its score.
/// The vector is sorted by score in descending order.
#[inline(always)]
pub fn predict_multiple(
    relationships: &[Relationship],
    current_position: usize,
    candles: &[TickerData],
) -> Vec<Prediction> {
    let mut scores = HashMap::new();
    for relationship in relationships {
        for d in 0..relationship.depth {
            let predict = candles[relationship.predict_index].candles()[current_position - d]
                .technicals()[relationship.r_type];
            let score = scores.entry(relationship.target_index).or_insert(0.0);
            *score += (predict * relationship.correlation).tanh();
        }
    }
    let mut predictions: Vec<_> = scores
        .into_iter()
        .map(|(target_index, score)| Prediction::new(target_index, score))
        .collect();
    predictions.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    predictions
}
