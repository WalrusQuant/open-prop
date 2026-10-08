//! Poisson-gamma rates. A player's per-minute rate has a gamma prior, so the
//! predictive count is a negative binomial: wider when the mean is higher, and
//! never below zero.

const LANCZOS: [f64; 9] = [
    0.99999999999980993,
    676.5203681218851,
    -1259.1392167224028,
    771.32342877765313,
    -176.61502916214059,
    12.507343278686905,
    -0.13857109526572012,
    9.9843695780195716e-6,
    1.5056327351493116e-7,
];

pub fn log_gamma(value: f64) -> f64 {
    if value < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * value).sin()).ln() - log_gamma(1.0 - value);
    }
    let z = value - 1.0;
    let mut series = LANCZOS[0];
    for (index, coeff) in LANCZOS.iter().enumerate().skip(1) {
        series += coeff / (z + index as f64);
    }
    let t = z + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + series.ln()
}

/// Marginal log likelihood of Poisson counts with exposures, gamma prior, up to terms
/// that cancel in a Bayes factor. `sum_y` is the total count. `sum_m` is the total exposure.
pub fn log_marginal(shape: f64, rate: f64, sum_y: f64, sum_m: f64) -> f64 {
    log_gamma(shape + sum_y) - log_gamma(shape) + shape * rate.ln()
        - (shape + sum_y) * (rate + sum_m).ln()
}

/// Posterior probability that the recent window is a new rate rather than the same one.
pub fn shift_probability(
    shape: f64,
    rate: f64,
    early_y: f64,
    early_m: f64,
    window_y: f64,
    window_m: f64,
    prior: f64,
) -> f64 {
    if !(prior > 0.0 && prior < 1.0) || early_m <= 0.0 || window_m <= 0.0 {
        return 0.0;
    }
    let log_bf = log_marginal(shape, rate, early_y, early_m)
        + log_marginal(shape, rate, window_y, window_m)
        - log_marginal(shape, rate, early_y + window_y, early_m + window_m);
    let logit = (log_bf + (prior / (1.0 - prior)).ln()).clamp(-40.0, 40.0);
    1.0 / (1.0 + (-logit).exp())
}

/// Negative binomial pmf for a gamma(shape, rate) rate and a known exposure.
/// Index `k` is P(Y = k). The slice is long enough that the missing tail is tiny.
pub fn negative_binomial(shape: f64, rate: f64, exposure: f64) -> Vec<f64> {
    if !(shape > 0.0) || !(rate > 0.0) {
        return vec![1.0];
    }
    if exposure <= 1e-8 {
        return vec![1.0];
    }
    let mean = exposure * shape / rate;
    let variance = mean + mean * mean / shape;
    let width = variance.sqrt();
    let len = (mean + 14.0 * width + 8.0).ceil() as usize;
    let len = len.clamp(2, 160);
    let mut pmf = vec![0.0; len];
    pmf[0] = (rate / (rate + exposure)).powf(shape);
    let step = exposure / (rate + exposure);
    for k in 0..len - 1 {
        pmf[k + 1] = pmf[k] * (shape + k as f64) / (k as f64 + 1.0) * step;
        if !pmf[k + 1].is_finite() {
            pmf[k + 1] = 0.0;
        }
    }
    pmf
}

pub fn mix(left: &[f64], right: &[f64], weight_right: f64) -> Vec<f64> {
    let weight = weight_right.clamp(0.0, 1.0);
    let len = left.len().max(right.len());
    let mut out = vec![0.0; len];
    for (index, slot) in out.iter_mut().enumerate() {
        let a = left.get(index).copied().unwrap_or(0.0);
        let b = right.get(index).copied().unwrap_or(0.0);
        *slot = (1.0 - weight) * a + weight * b;
    }
    out
}

pub fn convolve(left: &[f64], right: &[f64]) -> Vec<f64> {
    if left.is_empty() || right.is_empty() {
        return vec![];
    }
    let mut out = vec![0.0; left.len() + right.len() - 1];
    for (i, pa) in left.iter().enumerate() {
        if *pa <= 0.0 {
            continue;
        }
        for (j, pb) in right.iter().enumerate() {
            out[i + j] += pa * pb;
        }
    }
    trim(&mut out);
    out
}

pub fn scale_mix(components: &[Vec<f64>], weights: &[f64]) -> Vec<f64> {
    let len = components.iter().map(Vec::len).max().unwrap_or(1);
    let mut out = vec![0.0; len];
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return vec![1.0];
    }
    for (pmf, weight) in components.iter().zip(weights.iter()) {
        let share = weight / total;
        for (index, prob) in pmf.iter().enumerate() {
            out[index] += share * prob;
        }
    }
    out
}

fn trim(pmf: &mut Vec<f64>) {
    while pmf.len() > 2 && pmf.last().copied().unwrap_or(0.0) < 1e-12 {
        pmf.pop();
    }
}

pub fn mean(pmf: &[f64]) -> f64 {
    pmf.iter().enumerate().map(|(k, p)| k as f64 * p).sum()
}

pub fn at_least(pmf: &[f64], line: f64) -> f64 {
    if pmf.is_empty() {
        return 0.0;
    }
    let start = line.ceil().max(0.0) as usize;
    // 12.0 stays 12. 12.0000001 and 12.5 both start at 13, which is P(Y >= line) for an integer Y.
    let start = if (line - line.floor()).abs() < 1e-9 {
        line.max(0.0) as usize
    } else {
        start
    };
    pmf.iter().skip(start).sum()
}

/// Central band from the 10th to the 90th percentile.
pub fn band(pmf: &[f64]) -> (f64, f64) {
    if pmf.is_empty() {
        return (0.0, 0.0);
    }
    let mut cdf = 0.0;
    let mut low = 0usize;
    let mut high = pmf.len() - 1;
    let mut found_low = false;
    for (k, prob) in pmf.iter().enumerate() {
        cdf += prob;
        if !found_low && cdf >= 0.10 {
            low = k;
            found_low = true;
        }
        if cdf >= 0.90 {
            high = k;
            break;
        }
    }
    (low as f64, high.max(low) as f64)
}

pub fn std_dev(pmf: &[f64]) -> f64 {
    let center = mean(pmf);
    let second = pmf
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let d = k as f64 - center;
            d * d * p
        })
        .sum::<f64>();
    second.sqrt()
}

/// Lognormal minutes nodes. Weights are the normal density on the log scale.
pub fn minute_nodes(mean_minutes: f64, log_sd: f64) -> Vec<(f64, f64)> {
    let mean_minutes = mean_minutes.clamp(1.0, 42.0);
    let log_sd = log_sd.clamp(0.08, 0.55);
    let mu = mean_minutes.ln() - 0.5 * log_sd * log_sd;
    let steps = 15;
    let z_lo = -2.4;
    let z_hi = 2.4;
    let dz = (z_hi - z_lo) / (steps - 1) as f64;
    let mut nodes = Vec::with_capacity(steps);
    for index in 0..steps {
        let z = z_lo + index as f64 * dz;
        let density = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
        let minutes = (mu + log_sd * z).exp().clamp(0.0, 48.0);
        nodes.push((minutes, density * dz));
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_gamma_matches_factorials() {
        let value = log_gamma(6.0);
        // Gamma(6) = 5! = 120.
        let expected = 120f64.ln();
        assert!((value - expected).abs() < 1e-9, "{value} {expected}");
    }

    #[test]
    fn a_flat_window_stays_near_the_prior() {
        let prior = 0.1;
        let prob = shift_probability(80.0, 200.0, 400.0, 1000.0, 80.0, 200.0, prior);
        assert!(prob < prior, "{prob}");
    }

    #[test]
    fn a_different_window_moves_the_probability() {
        let prob = shift_probability(40.0, 200.0, 200.0, 640.0, 160.0, 160.0, 0.1);
        assert!(prob > 0.8, "{prob}");
    }

    #[test]
    fn negative_binomial_mean_matches_the_gamma() {
        let pmf = negative_binomial(40.0, 80.0, 30.0);
        let got = mean(&pmf);
        let expected = 30.0 * 40.0 / 80.0;
        assert!((got - expected).abs() < 0.05, "{got} {expected}");
        let total: f64 = pmf.iter().sum();
        assert!((total - 1.0).abs() < 1e-3, "{total}");
    }

    #[test]
    fn at_least_counts_the_integer_tail() {
        let pmf = vec![0.2, 0.2, 0.2, 0.2, 0.2];
        assert!((at_least(&pmf, 0.0) - 1.0).abs() < 1e-12);
        assert!((at_least(&pmf, 2.0) - 0.6).abs() < 1e-12);
        assert!((at_least(&pmf, 1.5) - 0.6).abs() < 1e-12);
    }

    /// vitest reads the same cases, so the player page's tail sum agrees with this one.
    #[test]
    fn at_least_matches_the_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/desk-math.json")).unwrap();
        let pmf: Vec<f64> = fixture["atLeast"]["pmf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_f64().unwrap())
            .collect();
        for case in fixture["atLeast"]["cases"].as_array().unwrap() {
            let line = case["line"].as_f64().unwrap();
            let expected = case["expected"].as_f64().unwrap();
            let actual = at_least(&pmf, line);
            assert!((actual - expected).abs() < 1e-9, "line {line}: {actual} vs {expected}");
        }
        let empty = &fixture["atLeastEmpty"];
        assert_eq!(at_least(&[], empty["line"].as_f64().unwrap()), 0.0);
    }

    #[test]
    fn convolution_adds_the_supports() {
        let sum = convolve(&[0.0, 1.0], &[0.0, 0.0, 1.0]);
        assert!((sum[3] - 1.0).abs() < 1e-12, "{sum:?}");
    }
}
