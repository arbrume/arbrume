//! Small dense linear algebra for the evolution strategy.

/// Eigen-decomposition of the symmetric `n` by `n` matrix `matrix` (row major) by the
/// cyclic Jacobi method. Returns the eigenvalues and the eigenvectors as the columns of
/// a row-major matrix. Deterministic: the sweeps visit the pairs in a fixed order.
pub fn symmetric_eigen(matrix: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut a = matrix.to_vec();
    let mut v = identity(n);
    for _ in 0..64 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i * n + j] * a[i * n + j])
            .sum();
        let scale: f64 = (0..n).map(|i| a[i * n + i] * a[i * n + i]).sum();
        if off <= 1e-30 * scale.max(f64::MIN_POSITIVE) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                rotate(&mut a, &mut v, n, p, q);
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

/// One Jacobi rotation that zeroes `matrix[p][q]`, accumulated into `vectors`.
fn rotate(matrix: &mut [f64], vectors: &mut [f64], n: usize, p: usize, q: usize) {
    let pivot = matrix[p * n + q];
    if pivot == 0.0 {
        return;
    }
    let theta = (matrix[q * n + q] - matrix[p * n + p]) / (2.0 * pivot);
    let tangent = theta.signum() / (theta.abs() + theta.mul_add(theta, 1.0).sqrt());
    let cos = 1.0 / tangent.mul_add(tangent, 1.0).sqrt();
    let sin = tangent * cos;
    let turn = |m: &mut [f64], first: usize, second: usize| {
        let (one, two) = (m[first], m[second]);
        m[first] = cos * one - sin * two;
        m[second] = sin * one + cos * two;
    };
    for k in 0..n {
        turn(matrix, k * n + p, k * n + q);
    }
    for k in 0..n {
        turn(matrix, p * n + k, q * n + k);
    }
    for k in 0..n {
        turn(vectors, k * n + p, k * n + q);
    }
}

/// The `n` by `n` identity, row major.
pub fn identity(n: usize) -> Vec<f64> {
    let mut m = vec![0.0; n * n];
    for i in 0..n {
        m[i * n + i] = 1.0;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::symmetric_eigen;

    #[test]
    fn eigenpairs_reconstruct_the_matrix() {
        let m = [4.0, 1.0, 0.5, 1.0, 3.0, 0.2, 0.5, 0.2, 1.0];
        let (values, vectors) = symmetric_eigen(&m, 3);
        for i in 0..3 {
            for j in 0..3 {
                let back: f64 = (0..3)
                    .map(|k| vectors[i * 3 + k] * values[k] * vectors[j * 3 + k])
                    .sum();
                assert!((back - m[i * 3 + j]).abs() < 1e-10);
            }
        }
    }
}
