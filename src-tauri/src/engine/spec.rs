use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::models::Stat;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSpec {
    pub stat: String,
    /// Pseudo-minutes in the role prior. Larger means a short sample shrinks harder.
    pub prior_minutes: f64,
    /// Pseudo-minutes of league-average production used to shrink an opponent factor toward 1.
    pub opponent_minutes: f64,
    /// Prior probability that the recent window is a new rate.
    pub shift_prior: f64,
    /// Minute cuts that separate bench, rotation, and starter. Two cuts make three roles.
    #[serde(default = "default_roles")]
    pub role_minutes: Vec<f64>,
    /// Prior standard deviation of the log home multiplier.
    #[serde(default = "default_home_sd")]
    pub home_sd: f64,
    /// Prior standard deviation of the log rest multiplier, per day of rest.
    #[serde(default = "default_rest_sd")]
    pub rest_sd: f64,
    /// Most pseudo-minutes of a player's last season that a seeded fit carries into his prior.
    /// Zero turns the player seed off.
    #[serde(default = "default_carry_minutes")]
    pub carry_minutes: f64,
    /// Pseudo-minutes that pull an opponent multiplier toward last season's value.
    #[serde(default = "default_opponent_carry_minutes")]
    pub opponent_carry_minutes: f64,
    /// Minutes scale for exp(-m/tau) decay of the player carry. 0 turns decay off.
    #[serde(default = "default_carry_decay_tau")]
    pub carry_decay_tau: f64,
}

fn default_roles() -> Vec<f64> {
    vec![15.0, 28.0]
}

fn default_home_sd() -> f64 {
    0.08
}

fn default_rest_sd() -> f64 {
    0.015
}

pub(crate) const DEFAULT_CARRY_MINUTES: f64 = 1000.0;

fn default_carry_minutes() -> f64 {
    DEFAULT_CARRY_MINUTES
}

fn default_opponent_carry_minutes() -> f64 {
    1500.0
}

fn default_carry_decay_tau() -> f64 {
    500.0
}

/// Where an edited spec is allowed to live. The first file that exists wins.
/// The compiled copies are the fallback when the app is launched outside the repo.
pub fn spec_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("models").join("specs"));
        dirs.push(cwd.join("..").join("models").join("specs"));
    }
    dirs.push(data_dir.join("specs"));
    dirs
}

pub fn load_spec(stat: &str, dirs: &[PathBuf]) -> AppResult<ModelSpec> {
    for dir in dirs {
        let path = dir.join(format!("{stat}.json"));
        if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            return parse_spec(&text);
        }
    }
    let embedded = embedded_spec(stat).ok_or_else(|| {
        AppError::message(format!("'{stat}' is not a stat this desk trains."))
    })?;
    parse_spec(embedded)
}

pub fn parse_spec(text: &str) -> AppResult<ModelSpec> {
    let spec: ModelSpec = serde_json::from_str(text)?;
    validate(&spec)?;
    Ok(spec)
}

pub(crate) fn validate(spec: &ModelSpec) -> AppResult<()> {
    if Stat::parse(&spec.stat).is_none() {
        return Err(AppError::message(format!(
            "'{}' is not a stat this desk trains.",
            spec.stat
        )));
    }
    if !(spec.prior_minutes > 0.0) || spec.prior_minutes > 5000.0 || !spec.prior_minutes.is_finite()
    {
        return Err(AppError::message(
            "prior_minutes has to be greater than 0 and at most 5000.".to_string(),
        ));
    }
    if !(spec.opponent_minutes > 0.0)
        || spec.opponent_minutes > 20000.0
        || !spec.opponent_minutes.is_finite()
    {
        return Err(AppError::message(
            "opponent_minutes has to be greater than 0 and at most 20000.".to_string(),
        ));
    }
    if !(spec.shift_prior > 0.0 && spec.shift_prior < 1.0) || !spec.shift_prior.is_finite() {
        return Err(AppError::message(
            "shift_prior has to be greater than 0 and less than 1.".to_string(),
        ));
    }
    if spec.role_minutes.is_empty() || spec.role_minutes.len() > 4 {
        return Err(AppError::message(
            "role_minutes needs between 1 and 4 cuts.".to_string(),
        ));
    }
    let mut last = 0.0;
    for cut in &spec.role_minutes {
        if !cut.is_finite() || *cut <= last || *cut >= 48.0 {
            return Err(AppError::message(
                "role_minutes cuts have to rise, and each one has to be between 0 and 48.".to_string(),
            ));
        }
        last = *cut;
    }
    if !(spec.home_sd > 0.0 && spec.home_sd <= 1.0) || !(spec.rest_sd > 0.0 && spec.rest_sd <= 0.2)
    {
        return Err(AppError::message(
            "home_sd has to be from 0 to 1, and rest_sd from 0 to 0.2.".to_string(),
        ));
    }
    if !spec.carry_minutes.is_finite() || !(0.0..=5000.0).contains(&spec.carry_minutes) {
        return Err(AppError::message(
            "carry_minutes has to be from 0 to 5000.".to_string(),
        ));
    }
    if !spec.opponent_carry_minutes.is_finite()
        || !(0.0..=20000.0).contains(&spec.opponent_carry_minutes)
    {
        return Err(AppError::message(
            "opponent_carry_minutes has to be from 0 to 20000.".to_string(),
        ));
    }
    if !spec.carry_decay_tau.is_finite() || !(0.0..=20000.0).contains(&spec.carry_decay_tau) {
        return Err(AppError::message(
            "carry_decay_tau has to be from 0 to 20000. Zero turns decay off.".to_string(),
        ));
    }
    Ok(())
}

fn embedded_spec(stat: &str) -> Option<&'static str> {
    Some(match stat {
        "points" => include_str!("../../../models/specs/points.json"),
        "rebounds" => include_str!("../../../models/specs/rebounds.json"),
        "assists" => include_str!("../../../models/specs/assists.json"),
        "steals" => include_str!("../../../models/specs/steals.json"),
        "blocks" => include_str!("../../../models/specs/blocks.json"),
        "turnovers" => include_str!("../../../models/specs/turnovers.json"),
        "field_goals_made" => include_str!("../../../models/specs/field_goals_made.json"),
        "field_goals_attempted" => include_str!("../../../models/specs/field_goals_attempted.json"),
        "three_point_field_goals_made" => {
            include_str!("../../../models/specs/three_point_field_goals_made.json")
        }
        "free_throws_made" => include_str!("../../../models/specs/free_throws_made.json"),
        "points_assists" => include_str!("../../../models/specs/points_assists.json"),
        "points_rebounds" => include_str!("../../../models/specs/points_rebounds.json"),
        "assists_rebounds" => include_str!("../../../models/specs/assists_rebounds.json"),
        "points_assists_rebounds" => {
            include_str!("../../../models/specs/points_assists_rebounds.json")
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_specs() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../models/specs")
    }

    #[test]
    fn every_stat_has_a_spec() {
        let dir = repo_specs();
        for stat in Stat::all() {
            let spec = load_spec(stat.id(), &[dir.clone()]).unwrap();
            assert_eq!(spec.stat, stat.id());
            assert!(spec.prior_minutes > 0.0, "{}", stat.id());
        }
    }

    #[test]
    fn carry_priors_default_and_stay_in_range() {
        let base = r#""stat": "points", "prior_minutes": 240, "opponent_minutes": 1500, "shift_prior": 0.1"#;
        let spec = parse_spec(&format!("{{{base}}}")).unwrap();
        assert_eq!(spec.carry_minutes, DEFAULT_CARRY_MINUTES);
        assert_eq!(spec.opponent_carry_minutes, 1500.0);
        let off = parse_spec(&format!(r#"{{{base}, "carry_minutes": 0, "opponent_carry_minutes": 0}}"#)).unwrap();
        assert_eq!((off.carry_minutes, off.opponent_carry_minutes), (0.0, 0.0));
        for bad in [
            r#""carry_minutes": -1"#,
            r#""carry_minutes": 5001"#,
            r#""opponent_carry_minutes": -5"#,
            r#""opponent_carry_minutes": 20001"#,
        ] {
            let error = parse_spec(&format!("{{{base}, {bad}}}")).unwrap_err();
            assert!(error.to_string().contains("carry_minutes"), "{bad}: {error}");
        }
    }

    #[test]
    fn a_shift_prior_of_one_is_rejected() {
        let text = r#"{
            "stat": "points",
            "prior_minutes": 240,
            "opponent_minutes": 1500,
            "shift_prior": 1
        }"#;
        let error = parse_spec(text).unwrap_err();
        assert!(error.to_string().contains("shift_prior"), "{error}");
    }
}
