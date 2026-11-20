//! Candidate intensities that give a target density after hard-core thinning.
//!
//! Candidates are a Poisson process; each gets a random priority and is removed when a
//! candidate of higher priority stands closer than the conflict distance of the pair:
//! Matérn's second hard-core process (Matérn 1960), with marks for the species. For a
//! candidate of species `s` with priority rank `u` (uniform in `[0, 1]`), the candidates
//! that beat it are a Poisson process of mean `(1 - u) L_s`, with
//! `L_s = sum_t lambda_t pi d_st^2`, so it survives with probability `exp(-(1 - u) L_s)`
//! and, over `u`, with probability `(1 - exp(-L_s)) / L_s`. A target density `rho_s` is
//! therefore reached by intensities with `lambda_s (1 - exp(-L_s)) / L_s = rho_s`,
//! solved here by fixed-point iteration from `lambda = rho`, which increases
//! monotonically to the smallest solution when one exists. A target is out of reach when
//! `rho_s pi d_ss^2 >= 1` for a single species: the thinned process cannot be denser
//! than `1 / (pi d^2)` however many candidates there are; the iteration then stops at an
//! intensity whose `L_s` is [`SATURATION`] and reports the species as saturated.

/// Largest `L_s`: beyond it `exp(-L_s)` is below `1e-13` and more candidates add nothing.
pub const SATURATION: f64 = 30.0;
/// Most iterations of the fixed point.
const ITERATIONS: usize = 1000;
/// Relative change under which the iteration stops.
const TOLERANCE: f64 = 1e-9;

/// `L / (1 - exp(-L))`, 1 at 0.
fn inverse_survival(load: f64) -> f64 {
    if load < 1e-9 {
        1.0 + load / 2.0
    } else {
        load / -(-load).exp_m1()
    }
}

/// Candidate intensities for the target densities `rho` (per square meter), with
/// `areas[s * n + t] = pi d_st^2`. Writes them into `lambda` and returns a bit per
/// species set when its target is out of reach.
pub fn intensities(rho: &[f64], areas: &[f64], lambda: &mut [f64]) -> u64 {
    let n = rho.len();
    lambda.copy_from_slice(rho);
    let mut saturated = 0u64;
    for _ in 0..ITERATIONS {
        let mut change: f64 = 0.0;
        for s in 0..n {
            if rho[s] <= 0.0 {
                lambda[s] = 0.0;
                continue;
            }
            let load: f64 = (0..n).map(|t| lambda[t] * areas[s * n + t]).sum();
            let mut next = rho[s] * inverse_survival(load.min(SATURATION));
            if load >= SATURATION {
                saturated |= 1 << s;
                next = lambda[s];
            }
            change = change.max((next - lambda[s]).abs() / next.max(1e-30));
            lambda[s] = next;
        }
        if change < TOLERANCE {
            break;
        }
    }
    saturated
}

/// The density the thinning leaves from intensities `lambda` (the inverse of
/// [`intensities`]).
#[must_use]
pub fn thinned(lambda: &[f64], areas: &[f64]) -> Vec<f64> {
    let n = lambda.len();
    (0..n)
        .map(|s| {
            let load: f64 = (0..n).map(|t| lambda[t] * areas[s * n + t]).sum();
            lambda[s] / inverse_survival(load)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{intensities, thinned};

    #[test]
    fn the_solution_thins_back_to_the_target() {
        let d: [f64; 4] = [4.0, 3.0, 3.0, 2.0];
        let areas: Vec<f64> = d.iter().map(|d| std::f64::consts::PI * d * d).collect();
        let rho = [0.01, 0.03];
        let mut lambda = [0.0; 2];
        assert_eq!(intensities(&rho, &areas, &mut lambda), 0);
        let back = thinned(&lambda, &areas);
        for (a, b) in back.iter().zip(rho) {
            assert!((a - b).abs() < 1e-9 * b, "{a} {b}");
        }
    }

    #[test]
    fn unreachable_targets_saturate() {
        let areas = [std::f64::consts::PI * 16.0];
        let mut lambda = [0.0];
        assert_eq!(intensities(&[0.05], &areas, &mut lambda), 1);
    }
}
