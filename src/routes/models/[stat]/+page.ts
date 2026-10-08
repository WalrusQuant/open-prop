export function entries() {
  return [
    "points",
    "rebounds",
    "assists",
    "steals",
    "blocks",
    "turnovers",
    "field_goals_made",
    "field_goals_attempted",
    "three_point_field_goals_made",
    "free_throws_made",
    "points_assists",
    "points_rebounds",
    "assists_rebounds",
    "points_assists_rebounds",
  ].map((stat) => ({ stat }));
}
