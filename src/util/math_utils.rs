/**
Calculates the median of a slice of f64 values.
Handles empty slices by returning NaN.
Handles slices with NaN values by potentially sorting them to the end (behavior depends on `sort_by`).

## Arguments
- `values`: A slice of f64 values.

## Returns
The median of the values, or `f64::NAN` if the slice is empty.
 */
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN; // Or return 0.0? NaN indicates undefined.
    }

    let mut sorted_values = values.to_vec();
    // Sort floats, handling NaNs (pushing them to one end, doesn't matter which for median)
    sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mid = sorted_values.len() / 2;
    if sorted_values.len() % 2 == 0 {
        // Even number of elements: average the two middle ones
        // Ensure the middle elements are finite before averaging
        let val1 = sorted_values[mid - 1];
        let val2 = sorted_values[mid];
        if val1.is_finite() && val2.is_finite() {
            (val1 + val2) / 2.0
        } else {
            f64::NAN // If either middle value is NaN, median is NaN
        }
    } else {
        // Odd number of elements: return the middle one
        sorted_values[mid] // This might be NaN if the middle element is NaN
    }
}


/**
Calculates the standard deviation of a slice of f64 values.

## Arguments
- `values`: A slice of f64 values.

## Returns
The standard deviation, or `None` if calculation is not possible (e.g., empty slice, contains NaNs).
 */
 pub fn std_deviation(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 2 { // Need at least 2 values for standard deviation
        return None;
    }

    // Check for NaNs or Infs
    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }


    let mean = values.iter().sum::<f64>() / (n as f64);

    let variance = values.iter()
        .map(|value| {
            let diff = mean - value;
            diff * diff
        })
        .sum::<f64>() / (n as f64); // Population variance, use n-1 for sample variance if needed

    Some(variance.sqrt())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_median_odd() {
        let values = vec![3.0, 1.0, 4.0, 1.0, 5.0];
        assert_eq!(median(&values), 3.0);
    }

    #[test]
    fn test_median_even() {
        let values = vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0];
        // Sorted: [1.0, 1.0, 3.0, 4.0, 5.0, 9.0] -> Middle are 3.0, 4.0. Median = 3.5
        assert_eq!(median(&values), 3.5);
    }

     #[test]
    fn test_median_empty() {
        let values: Vec<f64> = vec![];
        assert!(median(&values).is_nan());
    }

     #[test]
    fn test_median_single() {
        let values = vec![42.0];
        assert_eq!(median(&values), 42.0);
    }

     #[test]
    fn test_std_deviation_basic() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // Mean = 3.0
        // Variance = ((1-3)^2 + (2-3)^2 + (3-3)^2 + (4-3)^2 + (5-3)^2) / 5
        // Variance = (4 + 1 + 0 + 1 + 4) / 5 = 10 / 5 = 2.0
        // StdDev = sqrt(2.0) = 1.41421356...
        let std_dev = std_deviation(&values).unwrap();
        assert!((std_dev - 2.0f64.sqrt()).abs() < f64::EPSILON);
    }

     #[test]
    fn test_std_deviation_zero() {
        let values = vec![5.0, 5.0, 5.0];
        let std_dev = std_deviation(&values).unwrap();
        assert!(std_dev.abs() < f64::EPSILON);
    }

     #[test]
    fn test_std_deviation_insufficient_data() {
        let values = vec![5.0];
        assert!(std_deviation(&values).is_none());
        let values_empty: Vec<f64> = vec![];
        assert!(std_deviation(&values_empty).is_none());
    }

     #[test]
    fn test_std_deviation_with_nan() {
        let values = vec![1.0, 2.0, f64::NAN, 4.0, 5.0];
        assert!(std_deviation(&values).is_none());
    }
}