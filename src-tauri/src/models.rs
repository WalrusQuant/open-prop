use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub struct GameLog {
    pub player_id: i64,
    pub player_name: String,
    pub team_abbr: String,
    pub game_id: String,
    pub game_date: String,
    pub matchup: String,
    pub wl: String,
    pub minutes: f64,
    pub pts: i32,
    pub reb: i32,
    pub ast: i32,
    pub stl: i32,
    pub blk: i32,
    pub tov: i32,
    pub fgm: i32,
    pub fga: i32,
    pub fg3m: i32,
    pub ftm: i32,
    pub plus_minus: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    Points,
    Rebounds,
    Assists,
    Steals,
    Blocks,
    Turnovers,
    FieldGoalsMade,
    FieldGoalsAttempted,
    ThreePointersMade,
    FreeThrowsMade,
    PointsAssists,
    PointsRebounds,
    AssistsRebounds,
    PointsAssistsRebounds,
}

impl Stat {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "points" => Self::Points,
            "rebounds" => Self::Rebounds,
            "assists" => Self::Assists,
            "steals" => Self::Steals,
            "blocks" => Self::Blocks,
            "turnovers" => Self::Turnovers,
            "field_goals_made" => Self::FieldGoalsMade,
            "field_goals_attempted" => Self::FieldGoalsAttempted,
            "three_point_field_goals_made" => Self::ThreePointersMade,
            "free_throws_made" => Self::FreeThrowsMade,
            "points_assists" => Self::PointsAssists,
            "points_rebounds" => Self::PointsRebounds,
            "assists_rebounds" => Self::AssistsRebounds,
            "points_assists_rebounds" => Self::PointsAssistsRebounds,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Rebounds => "rebounds",
            Self::Assists => "assists",
            Self::Steals => "steals",
            Self::Blocks => "blocks",
            Self::Turnovers => "turnovers",
            Self::FieldGoalsMade => "field_goals_made",
            Self::FieldGoalsAttempted => "field_goals_attempted",
            Self::ThreePointersMade => "three_point_field_goals_made",
            Self::FreeThrowsMade => "free_throws_made",
            Self::PointsAssists => "points_assists",
            Self::PointsRebounds => "points_rebounds",
            Self::AssistsRebounds => "assists_rebounds",
            Self::PointsAssistsRebounds => "points_assists_rebounds",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Points => "Points",
            Self::Rebounds => "Rebounds",
            Self::Assists => "Assists",
            Self::Steals => "Steals",
            Self::Blocks => "Blocks",
            Self::Turnovers => "Turnovers",
            Self::FieldGoalsMade => "Field Goals Made",
            Self::FieldGoalsAttempted => "Field Goals Attempted",
            Self::ThreePointersMade => "Three Pointers Made",
            Self::FreeThrowsMade => "Free Throws Made",
            Self::PointsAssists => "Points + Assists",
            Self::PointsRebounds => "Points + Rebounds",
            Self::AssistsRebounds => "Assists + Rebounds",
            Self::PointsAssistsRebounds => "Points + Assists + Rebounds",
        }
    }

    pub fn value(self, game: &GameLog) -> f64 {
        match self {
            Self::Points => game.pts as f64,
            Self::Rebounds => game.reb as f64,
            Self::Assists => game.ast as f64,
            Self::Steals => game.stl as f64,
            Self::Blocks => game.blk as f64,
            Self::Turnovers => game.tov as f64,
            Self::FieldGoalsMade => game.fgm as f64,
            Self::FieldGoalsAttempted => game.fga as f64,
            Self::ThreePointersMade => game.fg3m as f64,
            Self::FreeThrowsMade => game.ftm as f64,
            Self::PointsAssists => (game.pts + game.ast) as f64,
            Self::PointsRebounds => (game.pts + game.reb) as f64,
            Self::AssistsRebounds => (game.ast + game.reb) as f64,
            Self::PointsAssistsRebounds => (game.pts + game.reb + game.ast) as f64,
        }
    }

    pub fn all() -> &'static [Stat] {
        &CATALOG
    }
}

const CATALOG: [Stat; 14] = [
    Stat::Points,
    Stat::Rebounds,
    Stat::Assists,
    Stat::Steals,
    Stat::Blocks,
    Stat::Turnovers,
    Stat::FieldGoalsMade,
    Stat::FieldGoalsAttempted,
    Stat::ThreePointersMade,
    Stat::FreeThrowsMade,
    Stat::PointsAssists,
    Stat::PointsRebounds,
    Stat::AssistsRebounds,
    Stat::PointsAssistsRebounds,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Last5,
    Last10,
    Last20,
    Season,
}

impl Window {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "last_5" => Self::Last5,
            "last_10" => Self::Last10,
            "last_20" => Self::Last20,
            "season" => Self::Season,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Last5 => "last_5",
            Self::Last10 => "last_10",
            Self::Last20 => "last_20",
            Self::Season => "season",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Last5 => "Last 5 games",
            Self::Last10 => "Last 10 games",
            Self::Last20 => "Last 20 games",
            Self::Season => "This season",
        }
    }

    pub fn all() -> &'static [Window] {
        &[Self::Last5, Self::Last10, Self::Last20, Self::Season]
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonStatus {
    pub season: String,
    pub season_type: String,
    pub games: i64,
    pub players: i64,
    pub first_game: Option<String>,
    pub last_game: Option<String>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub suggested_season: String,
    pub suggested_season_type: String,
    pub seasons: Vec<SeasonStatus>,
    pub stats: Vec<CatalogItem>,
    pub windows: Vec<CatalogItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub season: String,
    pub season_type: String,
    pub games: usize,
    pub players: usize,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerOption {
    pub player_id: i64,
    pub name: String,
    pub team: String,
    pub games: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendQuery {
    pub season: String,
    pub season_type: String,
    pub player_id: i64,
    pub stat: String,
    pub window: String,
    pub line: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendGame {
    pub game_id: String,
    pub game_date: String,
    pub matchup: String,
    pub opponent: String,
    pub location: String,
    pub result: String,
    pub minutes: f64,
    pub stat: f64,
    pub over: bool,
    pub moving_avg: Option<f64>,
    pub points: i32,
    pub rebounds: i32,
    pub assists: i32,
    pub steals: i32,
    pub blocks: i32,
    pub turnovers: i32,
    pub plus_minus: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendSummary {
    pub sample: usize,
    pub overs: usize,
    pub hit_rate: Option<f64>,
    pub wilson_low: Option<f64>,
    pub wilson_high: Option<f64>,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub sd: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendReport {
    pub player_id: i64,
    pub player_name: String,
    pub team: String,
    pub season: String,
    pub season_type: String,
    pub stat: String,
    pub stat_label: String,
    pub window: String,
    pub window_label: String,
    pub line: f64,
    pub games: Vec<TrendGame>,
    pub summary: TrendSummary,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardQuery {
    pub season: String,
    pub season_type: String,
    pub stat: String,
    pub min_games: u32,
    pub line: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSplit {
    pub overs: u32,
    pub games: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardRow {
    pub player_id: i64,
    pub name: String,
    pub team: String,
    pub games: usize,
    pub mean: f64,
    pub last5: BoardSplit,
    pub last10: BoardSplit,
    pub last20: BoardSplit,
    pub season: BoardSplit,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainQuery {
    pub season: String,
    pub season_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainStatReport {
    pub stat: String,
    pub label: String,
    pub error: Option<String>,
    pub train_rows: usize,
    pub holdout_rows: usize,
    pub holdout_mae: Option<f64>,
    pub baseline_mae: Option<f64>,
    pub holdout_coverage: Option<f64>,
    pub fitted_at: Option<String>,
    pub prior_minutes: Option<f64>,
    pub opponent_minutes: Option<f64>,
    pub shift_prior: Option<f64>,
    pub home_multiplier: Option<f64>,
    pub rest_per_day: Option<f64>,
    /// True when this fit stored its priors. False means the numbers are the spec on disk.
    pub settings_stored: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainReport {
    pub season: String,
    pub season_type: String,
    pub stats: Vec<TrainStatReport>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictQuery {
    pub season: String,
    pub season_type: String,
    pub player_id: i64,
    pub stat: String,
    pub line: f64,
    pub opponent: String,
    pub home: bool,
    pub rest_days: f64,
    /// Blank uses the shrunk last-10 minutes. A number collapses that distribution to one value.
    pub minutes: Option<f64>,
    /// `last_5`, `last_10`, `last_20`, or `season`. The season window does not run the shift test.
    #[serde(default = "default_window")]
    pub window: String,
}

fn default_window() -> String {
    "last_10".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    pub stat: String,
    pub stat_label: String,
    pub mean: f64,
    pub low: f64,
    pub high: f64,
    pub clear_probability: f64,
    /// Standard deviation of the predictive distribution.
    pub sigma: f64,
    /// Probability mass at 0, 1, 2, … The player page sums the tail when the line changes.
    pub pmf: Vec<f64>,
    /// Probability the selected window is a new rate. None for the season window and for combos.
    pub shift_probability: Option<f64>,
    pub minutes: f64,
    pub holdout_mae: Option<f64>,
    pub baseline_mae: Option<f64>,
    pub holdout_coverage: Option<f64>,
    pub train_rows: usize,
    pub holdout_rows: usize,
}
