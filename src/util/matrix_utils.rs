pub fn transpose(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let mut transposed = vec![];
    for i in 0..matrix[0].len() {
        let mut row = vec![];
        for row_element in &matrix {
            row.push(row_element[i]);
        }
        transposed.push(row);
    }
    transposed
}

// Normalize a matrix by rows using the z-score normalization method
pub fn normalize_by_rows(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let mut normalized = vec![];
    for row in matrix {
        let mean: f64 = row.iter().sum::<f64>() / row.len() as f64;
        let variance: f64 = row.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / row.len() as f64;
        let std_dev = variance.sqrt();
        let normalized_row: Vec<f64> = row.iter().map(|x| (x - mean) / std_dev).collect();
        normalized.push(normalized_row);
    }
    normalized
}

pub fn normalize_by_columns(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let transposed = transpose(matrix);
    let normalized = normalize_by_rows(transposed);
    transpose(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpose() {
        let matrix = vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]];
        let transposed = transpose(matrix);
        assert_eq!(
            transposed,
            vec![vec![1.0, 4.0], vec![2.0, 5.0], vec![3.0, 6.0]]
        );
    }

    #[test]
    fn test_large_matrix() {
        let matrix = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![6.0, 7.0, 8.0, 9.0, 10.0],
            vec![11.0, 12.0, 13.0, 14.0, 15.0],
            vec![16.0, 17.0, 18.0, 19.0, 20.0],
        ];
        let transposed = transpose(matrix);
        assert_eq!(
            transposed,
            vec![
                vec![1.0, 6.0, 11.0, 16.0],
                vec![2.0, 7.0, 12.0, 17.0],
                vec![3.0, 8.0, 13.0, 18.0],
                vec![4.0, 9.0, 14.0, 19.0],
                vec![5.0, 10.0, 15.0, 20.0]
            ]
        );
    }
}
