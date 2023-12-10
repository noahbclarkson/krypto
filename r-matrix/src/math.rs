pub fn max_index(vector: &[f64]) -> usize {
    let mut max_index = 0;
    let mut max_value = vector[0];
    for (index, value) in vector.iter().enumerate() {
        if *value > max_value {
            max_index = index;
            max_value = *value;
        }
    }
    max_index
}
