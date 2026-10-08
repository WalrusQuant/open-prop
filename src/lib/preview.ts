import { mean, median, movingAverage, sampleSd, wilson } from "./deskMath";
import type {
  BoardQuery,
  BoardRow,
  Bootstrap,
  CatalogItem,
  PlayerOption,
  TrendGame,
  TrendQuery,
  TrendReport,
} from "./types";

const STATS: CatalogItem[] = [
  { id: "points", label: "Points" },
  { id: "rebounds", label: "Rebounds" },
  { id: "assists", label: "Assists" },
  { id: "steals", label: "Steals" },
  { id: "blocks", label: "Blocks" },
  { id: "turnovers", label: "Turnovers" },
  { id: "field_goals_made", label: "Field Goals Made" },
  { id: "field_goals_attempted", label: "Field Goals Attempted" },
  { id: "three_point_field_goals_made", label: "Three Pointers Made" },
  { id: "free_throws_made", label: "Free Throws Made" },
  { id: "points_assists", label: "Points + Assists" },
  { id: "points_rebounds", label: "Points + Rebounds" },
  { id: "assists_rebounds", label: "Assists + Rebounds" },
  { id: "points_assists_rebounds", label: "Points + Assists + Rebounds" },
];

const WINDOWS: CatalogItem[] = [
  { id: "last_5", label: "Last 5 games" },
  { id: "last_10", label: "Last 10 games" },
  { id: "last_20", label: "Last 20 games" },
  { id: "season", label: "This season" },
];

interface Raw {
  playerId: number;
  name: string;
  team: string;
  date: string;
  opponent: string;
  home: boolean;
  result: string;
  minutes: number;
  points: number;
  rebounds: number;
  assists: number;
  steals: number;
  blocks: number;
  turnovers: number;
  fgm: number;
  fga: number;
  fg3m: number;
  ftm: number;
  plusMinus: number;
}

const PLAYERS = [
  { playerId: 1, name: "N. Sample", team: "LAB", bias: 27 },
  { playerId: 2, name: "A. Ledger", team: "LAB", bias: 22 },
  { playerId: 3, name: "M. Margin", team: "INK", bias: 18 },
  { playerId: 4, name: "R. Interval", team: "INK", bias: 14 },
  { playerId: 5, name: "C. Count", team: "SET", bias: 11 },
  { playerId: 6, name: "P. Width", team: "SET", bias: 8 },
];

const OPPONENTS = ["DAL", "BOS", "NYK", "LAL", "MIL", "DEN", "MIA", "PHX"];

function unit(seed: number): number {
  const value = Math.sin(seed * 12.9898) * 43758.5453;
  return value - Math.floor(value);
}

function buildRows(): Raw[] {
  const rows: Raw[] = [];
  for (const player of PLAYERS) {
    const games = player.playerId === 1 ? 24 : 22;
    for (let index = 0; index < games; index += 1) {
      const noise = unit(player.playerId * 100 + index);
      const date = new Date(2025, 10, 1 + index * 2 - Math.floor(index / 4));
      const iso = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
      const points = Math.max(2, Math.round(player.bias + (noise - 0.42) * 16));
      rows.push({
        playerId: player.playerId,
        name: player.name,
        team: player.team,
        date: iso,
        opponent: OPPONENTS[index % OPPONENTS.length],
        home: index % 2 === 0,
        result: noise > 0.45 ? "W" : "L",
        minutes: Math.round((28 + noise * 10) * 10) / 10,
        points,
        rebounds: Math.max(0, Math.round(player.bias / 5 + noise * 6)),
        assists: Math.max(0, Math.round(player.bias / 6 + (1 - noise) * 5)),
        steals: Math.round(noise * 3),
        blocks: Math.round((1 - noise) * 2),
        turnovers: 1 + Math.round(noise * 3),
        fgm: Math.round(points * 0.38),
        fga: Math.round(points * 0.8),
        fg3m: Math.round(noise * 4),
        ftm: Math.max(0, points - Math.round(points * 0.38) * 2),
        plusMinus: Math.round((noise - 0.5) * 24),
      });
    }
  }
  return rows;
}

const ROWS = buildRows();

function statValue(row: Raw, stat: string): number {
  switch (stat) {
    case "points":
      return row.points;
    case "rebounds":
      return row.rebounds;
    case "assists":
      return row.assists;
    case "steals":
      return row.steals;
    case "blocks":
      return row.blocks;
    case "turnovers":
      return row.turnovers;
    case "field_goals_made":
      return row.fgm;
    case "field_goals_attempted":
      return row.fga;
    case "three_point_field_goals_made":
      return row.fg3m;
    case "free_throws_made":
      return row.ftm;
    case "points_assists":
      return row.points + row.assists;
    case "points_rebounds":
      return row.points + row.rebounds;
    case "assists_rebounds":
      return row.assists + row.rebounds;
    case "points_assists_rebounds":
      return row.points + row.rebounds + row.assists;
    default:
      return row.points;
  }
}

function windowSize(windowId: string): number | null {
  if (windowId === "last_5") return 5;
  if (windowId === "last_10") return 10;
  if (windowId === "last_20") return 20;
  return null;
}

export function previewBootstrap(): Bootstrap {
  return {
    suggestedSeason: "2025-26",
    suggestedSeasonType: "Regular Season",
    seasons: ["2024-25", "2025-26", "2026-27"].flatMap((season) =>
      ["Regular Season", "Playoffs"].map((seasonType) => ({
        season,
        seasonType,
        games: season === "2025-26" && seasonType === "Regular Season" ? ROWS.length : 0,
        players:
          season === "2025-26" && seasonType === "Regular Season"
            ? new Set(ROWS.map((row) => row.playerId)).size
            : 0,
        firstGame:
          season === "2025-26" && seasonType === "Regular Season"
            ? [...ROWS].sort((left, right) => left.date.localeCompare(right.date))[0]?.date ?? null
            : null,
        lastGame:
          season === "2025-26" && seasonType === "Regular Season"
            ? [...ROWS].sort((left, right) => right.date.localeCompare(left.date))[0]?.date ?? null
            : null,
        syncedAt: season === "2025-26" && seasonType === "Regular Season" ? "2026-04-13T18:00:00Z" : null,
      })),
    ),
    stats: STATS,
    windows: WINDOWS,
  };
}

export function previewPlayers(): PlayerOption[] {
  return PLAYERS.map((player) => ({
    playerId: player.playerId,
    name: player.name,
    team: player.team,
    games: ROWS.filter((row) => row.playerId === player.playerId).length,
  }));
}

export function previewTrend(query: TrendQuery): TrendReport {
  const label = STATS.find((item) => item.id === query.stat)?.label ?? query.stat;
  const windowLabel = WINDOWS.find((item) => item.id === query.window)?.label ?? query.window;
  const owned = ROWS.filter((row) => row.playerId === query.playerId).sort((left, right) =>
    left.date.localeCompare(right.date),
  );
  const size = windowSize(query.window);
  const slice = size == null ? owned : owned.slice(-size);
  const values = slice.map((row) => statValue(row, query.stat));
  const averages = movingAverage(values, 3);
  const overs = values.filter((value) => value >= query.line).length;
  const interval = wilson(overs, values.length);
  const player = PLAYERS.find((item) => item.playerId === query.playerId);
  const games: TrendGame[] = slice.map((row, index) => ({
    gameId: `${row.playerId}-${row.date}`,
    gameDate: row.date,
    matchup: row.home ? `${row.team} vs. ${row.opponent}` : `${row.team} @ ${row.opponent}`,
    opponent: row.opponent,
    location: row.home ? "home" : "away",
    result: row.result,
    minutes: row.minutes,
    stat: values[index],
    over: values[index] >= query.line,
    movingAvg: averages[index],
    points: row.points,
    rebounds: row.rebounds,
    assists: row.assists,
    steals: row.steals,
    blocks: row.blocks,
    turnovers: row.turnovers,
    plusMinus: row.plusMinus,
  }));
  return {
    playerId: query.playerId,
    playerName: player?.name ?? "Unknown",
    team: player?.team ?? "",
    season: query.season,
    seasonType: query.seasonType,
    stat: query.stat,
    statLabel: label,
    window: query.window,
    windowLabel,
    line: query.line,
    games,
    summary: {
      sample: values.length,
      overs,
      hitRate: values.length ? overs / values.length : null,
      wilsonLow: interval?.low ?? null,
      wilsonHigh: interval?.high ?? null,
      mean: mean(values),
      median: median(values),
      sd: sampleSd(values),
      min: values.length ? Math.min(...values) : null,
      max: values.length ? Math.max(...values) : null,
      dnp: 0,
    },
  };
}

function boardSplit(values: number[], line: number, count: number | null) {
  const slice = count == null || values.length <= count ? values : values.slice(-count);
  return { overs: slice.filter((value) => value >= line).length, games: slice.length };
}

export function previewBoard(query: BoardQuery): BoardRow[] {
  const rows: BoardRow[] = [];
  for (const player of PLAYERS) {
    const values = ROWS.filter((row) => row.playerId === player.playerId)
      .sort((left, right) => left.date.localeCompare(right.date))
      .map((row) => statValue(row, query.stat));
    if (values.length < query.minGames) continue;
    const last10 = boardSplit(values, query.line, 10);
    rows.push({
      playerId: player.playerId,
      name: player.name,
      team: player.team,
      games: values.length,
      dnp: 0,
      mean: mean(values) ?? 0,
      last5: boardSplit(values, query.line, 5),
      last10,
      last20: boardSplit(values, query.line, 20),
      season: boardSplit(values, query.line, null),
    });
  }
  return rows.sort((left, right) => {
    const leftRate = left.last10.games ? left.last10.overs / left.last10.games : 0;
    const rightRate = right.last10.games ? right.last10.overs / right.last10.games : 0;
    return rightRate - leftRate || right.last10.overs - left.last10.overs || left.name.localeCompare(right.name);
  });
}
