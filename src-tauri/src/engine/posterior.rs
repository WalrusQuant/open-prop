/// z such that a standard normal leaves 10% in each tail. The middle 80% is mean ± this.
pub const Z80: f64 = 1.2815515655446004;

/// Inverse-gamma prior strength. Two pseudo-observations, so a season of residuals dominates.
const PRIOR_ALPHA: f64 = 2.0;

/// Posterior mean of sigma for a normal residual, given a weak inverse-gamma prior.
///
/// The prior mean of sigma is `prior_sigma`. Empty residuals return that prior.
pub fn residual_sigma(residuals: &[f64], prior_sigma: f64) -> f64 {
    let beta0 = prior_sigma * prior_sigma * (PRIOR_ALPHA - 1.0);
    let count = residuals.len() as f64;
    let rss = residuals.iter().map(|residual| residual * residual).sum::<f64>();
    let alpha = PRIOR_ALPHA + count / 2.0;
    let beta = beta0 + 0.5 * rss;
    (beta / (alpha - 1.0)).sqrt()
}

/// Abramowitz and Stegun 7.1.26. Maximum error about 1.5e-7.
fn erf(value: f64) -> f64 {
    let sign = if value < 0.0 { -1.0 } else { 1.0 };
    let value = value.abs();
    let t = 1.0 / (1.0 + 0.3275911 * value);
    let polynomial = (((((1.061405429 * t) - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
        + 0.254829592;
    sign * (1.0 - polynomial * t * (-value * value).exp())
}

pub fn normal_cdf(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

/// P(stat >= line) under Normal(mean, sigma). A vanished sigma is a point mass.
pub fn clear_probability(mean: f64, sigma: f64, line: f64) -> f64 {
    if !sigma.is_finite() || sigma <= 1e-9 {
        return if mean >= line { 1.0 } else { 0.0 };
    }
    (1.0 - normal_cdf((line - mean) / sigma)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cdf_hits_the_80_percent_marks() {
        let center = normal_cdf(0.0);
        assert!((center - 0.5).abs() < 1e-6, "{center}");
        let upper = normal_cdf(Z80);
        assert!((upper - 0.9).abs() < 1e-4, "{upper}");
    }

    #[test]
    fn empty_residuals_keep_the_prior_sigma() {
        let sigma = residual_sigma(&[], 5.0);
        assert!((sigma - 5.0).abs() < 1e-9, "{sigma}");
    }

    #[test]
    fn zero_residuals_shrink_sigma_toward_the_data() {
        // alpha = 2 + 2/2 = 3, beta = 25 + 0, sigma^2 = 25 / 2.
        let sigma = residual_sigma(&[0.0, 0.0], 5.0);
        let expected = (12.5_f64).sqrt();
        assert!((sigma - expected).abs() < 1e-9, "{sigma}");
    }

    #[test]
    fn clear_probability_is_half_when_the_line_is_the_mean() {
        let probability = clear_probability(20.0, 4.0, 20.0);
        assert!((probability - 0.5).abs() < 1e-6, "{probability}");
    }
}
