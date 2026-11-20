//! The covariance matrix adaptation evolution strategy, CMA-ES (Hansen 2016).
//!
//! This is the `(mu/mu_w, lambda)`-CMA-ES of Hansen's tutorial ("The CMA Evolution
//! Strategy: A Tutorial", arXiv:1604.00772, 2016) with its default settings: population
//! `lambda = 4 + floor(3 ln n)`, `mu = floor(lambda / 2)` positive recombination weights
//! `ln((lambda + 1) / 2) - ln i` normalised to sum 1, cumulation `c_c = (4 + mu_eff / n) /
//! (n + 4 + 2 mu_eff / n)`, `c_sigma = (mu_eff + 2) / (n + mu_eff + 5)`, rank-one rate
//! `c_1 = 2 / ((n + 1.3)^2 + mu_eff)`, rank-mu rate `c_mu = min(1 - c_1, 2 (mu_eff - 2 +
//! 1 / mu_eff) / ((n + 2)^2 + mu_eff))`, damping `d_sigma = 1 + 2 max(0, sqrt((mu_eff - 1)
//! / (n + 1)) - 1) + c_sigma`, and the stall indicator `h_sigma` of its equation (45).
//! The covariance is decomposed at every generation (the dimensions here are small).
//!
//! Sampling is deterministic: candidate `k` of generation `g` draws its standard normal
//! vector from the counter-based generator of `arbrume-core` keyed by the seed, `g` and
//! `k`, with the Box-Muller transform in double precision. The whole state serialises to
//! JSON, so a run can stop after any generation and resume to the same result.

use arbrume_core::rng::{Rng, Stream, mix64};
use serde::{Deserialize, Serialize};

use crate::linalg::{identity, symmetric_eigen};

/// Stream of the sampling draws.
const SAMPLING: Stream = Stream::custom(0x200);

/// Settings of [`Cmaes`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CmaesSettings {
    /// Population size; 0 for the default `4 + floor(3 ln n)`.
    pub lambda: usize,
    /// Initial step size.
    pub sigma: f64,
    /// Seed of the sampling.
    pub seed: u64,
}

/// State of a CMA-ES run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cmaes {
    n: usize,
    lambda: usize,
    weights: Vec<f64>,
    mueff: f64,
    cc: f64,
    cs: f64,
    c1: f64,
    cmu: f64,
    damps: f64,
    chi_n: f64,
    seed: u64,
    /// Mean of the search distribution.
    pub mean: Vec<f64>,
    /// Step size.
    pub sigma: f64,
    cov: Vec<f64>,
    pc: Vec<f64>,
    ps: Vec<f64>,
    basis: Vec<f64>,
    scales: Vec<f64>,
    /// Generations told so far.
    pub generation: u64,
}

impl Cmaes {
    /// A run starting at `mean` with the identity covariance.
    #[must_use]
    pub fn new(mean: Vec<f64>, settings: &CmaesSettings) -> Self {
        let n = mean.len().max(1);
        let nf = n as f64;
        let lambda = if settings.lambda > 0 {
            settings.lambda.max(2)
        } else {
            4 + (3.0 * nf.ln()).floor() as usize
        };
        let mu = lambda / 2;
        let raw: Vec<f64> = (1..=mu)
            .map(|i| f64::midpoint(lambda as f64, 1.0).ln() - (i as f64).ln())
            .collect();
        let total: f64 = raw.iter().sum();
        let weights: Vec<f64> = raw.iter().map(|w| w / total).collect();
        let mueff = 1.0 / weights.iter().map(|w| w * w).sum::<f64>();
        let cc = (4.0 + mueff / nf) / (nf + 4.0 + 2.0 * mueff / nf);
        let cs = (mueff + 2.0) / (nf + mueff + 5.0);
        let c1 = 2.0 / ((nf + 1.3).powi(2) + mueff);
        let cmu = (1.0 - c1).min(2.0 * (mueff - 2.0 + 1.0 / mueff) / ((nf + 2.0).powi(2) + mueff));
        let damps = 1.0 + 2.0 * (((mueff - 1.0) / (nf + 1.0)).sqrt() - 1.0).max(0.0) + cs;
        let chi_n = nf.sqrt() * (1.0 - 1.0 / (4.0 * nf) + 1.0 / (21.0 * nf * nf));
        Self {
            n: mean.len(),
            lambda,
            weights,
            mueff,
            cc,
            cs,
            c1,
            cmu,
            damps,
            chi_n,
            seed: settings.seed,
            mean,
            sigma: settings.sigma,
            cov: identity(n),
            pc: vec![0.0; n],
            ps: vec![0.0; n],
            basis: identity(n),
            scales: vec![1.0; n],
            generation: 0,
        }
    }

    /// Population size.
    #[must_use]
    pub const fn lambda(&self) -> usize {
        self.lambda
    }

    /// Dimension.
    #[must_use]
    pub const fn dimension(&self) -> usize {
        self.n
    }

    /// Largest standard deviation of the distribution along a coordinate.
    #[must_use]
    pub fn max_std(&self) -> f64 {
        let n = self.n;
        let largest = (0..n).map(|i| self.cov[i * n + i]).fold(0.0, f64::max);
        self.sigma * largest.sqrt()
    }

    /// Standard deviation of the distribution along each coordinate.
    #[must_use]
    pub fn stds(&self) -> Vec<f64> {
        (0..self.n)
            .map(|i| self.sigma * self.cov[i * self.n + i].sqrt())
            .collect()
    }

    /// The `lambda` candidates of the current generation.
    #[must_use]
    pub fn ask(&self) -> Vec<Vec<f64>> {
        let n = self.n;
        let key = mix64(self.seed ^ mix64(self.generation));
        (0..self.lambda)
            .map(|k| {
                let mut rng = Rng::new(key, k as u32, SAMPLING);
                let z: Vec<f64> = (0..n).map(|_| normal(&mut rng)).collect();
                (0..n)
                    .map(|i| {
                        let y: f64 = (0..n)
                            .map(|j| self.basis[i * n + j] * self.scales[j] * z[j])
                            .sum();
                        self.mean[i] + self.sigma * y
                    })
                    .collect()
            })
            .collect()
    }

    /// Updates the distribution from the candidates of [`Cmaes::ask`] and their values
    /// (smaller is better). Ties are broken by candidate index.
    ///
    /// # Panics
    ///
    /// Panics when the number of candidates or of values is not `lambda`.
    pub fn tell(&mut self, candidates: &[Vec<f64>], values: &[f64]) {
        assert_eq!(candidates.len(), self.lambda);
        assert_eq!(values.len(), self.lambda);
        let n = self.n;
        let mut ranked: Vec<usize> = (0..self.lambda).collect();
        ranked.sort_by(|&a, &b| values[a].total_cmp(&values[b]).then(a.cmp(&b)));
        let steps: Vec<Vec<f64>> = ranked
            .iter()
            .take(self.weights.len())
            .map(|&k| {
                (0..n)
                    .map(|i| (candidates[k][i] - self.mean[i]) / self.sigma)
                    .collect()
            })
            .collect();
        let yw: Vec<f64> = (0..n)
            .map(|i| self.weights.iter().zip(&steps).map(|(w, y)| w * y[i]).sum())
            .collect();
        for i in 0..n {
            self.mean[i] += self.sigma * yw[i];
        }
        let correction = self.update_paths(&yw);
        self.update_covariance(&steps, correction);
        let norm = self.ps.iter().map(|v| v * v).sum::<f64>().sqrt();
        self.sigma *= ((self.cs / self.damps) * (norm / self.chi_n - 1.0)).exp();
        self.generation += 1;
        self.decompose();
    }

    /// Cumulates the evolution paths with the mean step `yw`. Returns the correction
    /// `(1 - h_sigma) c_1 c_c (2 - c_c)` of the covariance decay.
    fn update_paths(&mut self, yw: &[f64]) -> f64 {
        let n = self.n;
        let whitened: Vec<f64> = (0..n)
            .map(|j| (0..n).map(|i| self.basis[i * n + j] * yw[i]).sum::<f64>() / self.scales[j])
            .collect();
        let factor = (self.cs * (2.0 - self.cs) * self.mueff).sqrt();
        for i in 0..n {
            let c: f64 = (0..n).map(|j| self.basis[i * n + j] * whitened[j]).sum();
            self.ps[i] = (1.0 - self.cs) * self.ps[i] + factor * c;
        }
        let norm = self.ps.iter().map(|v| v * v).sum::<f64>().sqrt();
        let decay = 1.0 - (1.0 - self.cs).powf(2.0 * (self.generation + 1) as f64);
        let stalled = norm / decay.sqrt() >= (1.4 + 2.0 / (n as f64 + 1.0)) * self.chi_n;
        let h = if stalled { 0.0 } else { 1.0 };
        let factor = h * (self.cc * (2.0 - self.cc) * self.mueff).sqrt();
        for i in 0..n {
            self.pc[i] = (1.0 - self.cc) * self.pc[i] + factor * yw[i];
        }
        (1.0 - h) * self.c1 * self.cc * (2.0 - self.cc)
    }

    /// Rank-one and rank-mu update of the covariance with the selected `steps`.
    fn update_covariance(&mut self, steps: &[Vec<f64>], correction: f64) {
        let n = self.n;
        let keep = 1.0 - self.c1 - self.cmu + correction;
        for i in 0..n {
            for j in 0..n {
                let rank_mu: f64 = self
                    .weights
                    .iter()
                    .zip(steps)
                    .map(|(w, y)| w * y[i] * y[j])
                    .sum();
                let rank_one = self.pc[i] * self.pc[j];
                self.cov[i * n + j] =
                    keep * self.cov[i * n + j] + self.c1 * rank_one + self.cmu * rank_mu;
            }
        }
        for i in 0..n {
            for j in 0..i {
                let average = f64::midpoint(self.cov[i * n + j], self.cov[j * n + i]);
                self.cov[i * n + j] = average;
                self.cov[j * n + i] = average;
            }
        }
    }

    /// Recomputes the eigenbasis and the scales (square roots of the eigenvalues).
    fn decompose(&mut self) {
        let (values, vectors) = symmetric_eigen(&self.cov, self.n);
        self.basis = vectors;
        self.scales = values.iter().map(|v| v.max(1e-300).sqrt()).collect();
    }
}

/// Standard normal deviate in double precision (Box-Muller).
fn normal(rng: &mut Rng) -> f64 {
    let uniform =
        |rng: &mut Rng| ((rng.next_u64() >> 11) as f64 + 1.0) * (1.0 / 9_007_199_254_740_992.0);
    let radius = (-2.0 * uniform(rng).ln()).sqrt();
    radius * (std::f64::consts::TAU * uniform(rng)).cos()
}
