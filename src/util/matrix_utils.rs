/**
Transpose a matrix (Vec<Vec<f64>>). Rows become columns and vice versa.
Assumes the input matrix is non-empty and rectangular.

## Arguments
- `matrix`: A matrix (Vec of rows) of f64 values.

## Returns
A new matrix representing the transpose of the input. Returns empty Vec if input is empty.
*/
pub fn transpose(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    if matrix.is_empty() || matrix[0].is_empty() {
        return Vec::new();
    }

    let num_rows = matrix.len();
    let num_cols = matrix[0].len();
    let mut transposed = vec![vec![0.0; num_rows]; num_cols]; // Pre-allocate with correct size

    for (i, row) in matrix.iter().enumerate() {
        // Add check for rectangularity?
        // if row.len() != num_cols { /* handle error */ }
        for (j, &val) in row.iter().enumerate().take(num_cols) {
            transposed[j][i] = val;
        }
    }
    transposed
}

/**
Normalize a matrix by rows. Each row will have a mean of 0 and a standard deviation of 1.
Handles rows with zero standard deviation by setting all elements to 0.0.
Handles NaNs/Infs in input by potentially propagating them or causing errors in mean/std dev calc.
It's better to clean data *before* calling this.

## Arguments
- `matrix`: A matrix (Vec of rows) of f64 values.

## Returns
A new matrix with each row normalized.
*/
pub fn normalize_by_rows(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    matrix
        .into_iter()
        .map(|row| {
            let len = row.len() as f64;
            if len == 0.0 {
                return vec![];
            } // Handle empty row

            let mean: f64 = row.iter().sum::<f64>() / len;

            // Check if mean is finite (input might contain NaN/Inf)
            if !mean.is_finite() {
                // Return row of NaNs or zeros? Zeros might be safer for downstream.
                return vec![0.0; row.len()];
            }

            let variance: f64 = row.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / len;
            let std_dev = variance.sqrt();

            if !std_dev.is_finite() {
                // Variance calculation failed (likely due to NaN/Inf in input)
                return vec![0.0; row.len()];
            }

            if std_dev.abs() < f64::EPSILON {
                // Zero standard deviation, return row of zeros
                vec![0.0; row.len()]
            } else {
                row.iter().map(|x| (x - mean) / std_dev).collect()
            }
        })
        .collect()
}

/**
Normalize a matrix by columns. Each column will have a mean of 0 and a standard deviation of 1.
Achieved by transposing, normalizing rows, and transposing back.

## Arguments
- `matrix`: A matrix (Vec of rows) of f64 values.

## Returns
A new matrix with each column normalized.
*/
pub fn normalize_by_columns(matrix: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    if matrix.is_empty() || matrix[0].is_empty() {
        return Vec::new();
    }
    // TODO: Check for rectangularity before transposing?
    let transposed = transpose(matrix);
    let normalized_rows = normalize_by_rows(transposed);
    transpose(normalized_rows) // Transpose back
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to compare float vecs
    fn assert_vec_eq(a: &[Vec<f64>], b: &[Vec<f64>]) {
        assert_eq!(a.len(), b.len());
        for i in 0..a.len() {
            assert_eq!(a[i].len(), b[i].len());
            for j in 0..a[i].len() {
                assert!(
                    (a[i][j] - b[i][j]).abs() < 1e-9,
                    "Mismatch at [{}][{}]: {} != {}",
                    i,
                    j,
                    a[i][j],
                    b[i][j]
                );
            }
        }
    }

    #[test]
    fn test_transpose_basic() {
        let matrix = vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]];
        let expected = vec![vec![1.0, 4.0], vec![2.0, 5.0], vec![3.0, 6.0]];
        assert_vec_eq(&transpose(matrix), &expected);
    }

    #[test]
    fn test_transpose_empty() {
        let matrix: Vec<Vec<f64>> = vec![];
        let expected: Vec<Vec<f64>> = vec![];
        assert_vec_eq(&transpose(matrix), &expected);

        let matrix_empty_row = vec![vec![]];
        let expected_empty_row: Vec<Vec<f64>> = vec![]; // Transpose of [[]] is []
        assert_vec_eq(&transpose(matrix_empty_row), &expected_empty_row);
    }

    #[test]
    fn test_normalize_rows_basic() {
        let matrix = vec![
            vec![1.0, 2.0, 3.0], // Mean 2, Var 2/3, StdDev sqrt(2/3)
            vec![5.0, 5.0, 5.0], // Mean 5, StdDev 0
        ];
        let std_dev1 = (2.0 / 3.0f64).sqrt();
        let expected = vec![
            vec![
                (1.0 - 2.0) / std_dev1,
                (2.0 - 2.0) / std_dev1,
                (3.0 - 2.0) / std_dev1,
            ],
            vec![0.0, 0.0, 0.0],
        ];
        assert_vec_eq(&normalize_by_rows(matrix), &expected);
    }

    #[test]
    fn test_normalize_rows_with_nan() {
        let matrix = vec![
            vec![1.0, 2.0, f64::NAN], // Should result in zeros due to non-finite mean/var
            vec![5.0, 5.0, 5.0],
        ];
        let expected = vec![
            vec![0.0, 0.0, 0.0], // Expect zeros as normalization fails
            vec![0.0, 0.0, 0.0],
        ];
        assert_vec_eq(&normalize_by_rows(matrix), &expected);
    }

    #[test]
    fn test_normalize_columns_basic() {
        let matrix = vec![
            vec![1.0, 6.0], // Col1 Mean=3, Col2 Mean=6
            vec![5.0, 6.0], // Col1 StdDev=sqrt(((1-3)^2+(5-3)^2)/2)=sqrt(4)=2
                            // Col2 StdDev=0
        ];
        // Expected Col1: (1-3)/2 = -1.0, (5-3)/2 = 1.0
        // Expected Col2: 0.0, 0.0
        let expected = vec![vec![-1.0, 0.0], vec![1.0, 0.0]];
        assert_vec_eq(&normalize_by_columns(matrix), &expected);
    }

    #[test]
    fn test_normalize_columns_empty() {
        let matrix: Vec<Vec<f64>> = vec![];
        let expected: Vec<Vec<f64>> = vec![];
        assert_vec_eq(&normalize_by_columns(matrix), &expected);
    }

    #[test]
    fn test_large_matrix_transpose() {
        // Renamed from test_large_matrix
        let matrix = vec![
            vec![1.0, 2.0, 3.0, 4.0, 5.0],
            vec![6.0, 7.0, 8.0, 9.0, 10.0],
            vec![11.0, 12.0, 13.0, 14.0, 15.0],
            vec![16.0, 17.0, 18.0, 19.0, 20.0],
        ];
        let expected = vec![
            vec![1.0, 6.0, 11.0, 16.0],
            vec![2.0, 7.0, 12.0, 17.0],
            vec![3.0, 8.0, 13.0, 18.0],
            vec![4.0, 9.0, 14.0, 19.0],
            vec![5.0, 10.0, 15.0, 20.0],
        ];
        assert_vec_eq(&transpose(matrix), &expected);
    }
}
