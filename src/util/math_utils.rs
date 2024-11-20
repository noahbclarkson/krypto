pub fn median(values: &[f64]) -> f64 {
    let mut sorted_values = values.to_vec();
    sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = sorted_values.len() / 2;
    match sorted_values.len() % 2 {
        0 => (sorted_values[mid - 1] + sorted_values[mid]) / 2.0,
        _ => sorted_values[mid],
    }
}
