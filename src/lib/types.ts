export interface CatalogItem {
  id: string;
  label: string;
}

export interface SeasonStatus {
  season: string;
  seasonType: string;
  games: number;
  players: number;
  firstGame: string | null;
  lastGame: string | null;
  syncedAt: string | null;
}

export interface Bootstrap {
  suggestedSeason: string;
  suggestedSeasonType: string;
  seasons: SeasonStatus[];
  stats: CatalogItem[];
  windows: CatalogItem[];
}

export interface SyncReport {
  season: string;
  seasonType: string;
  /** Games and players in the cache after the sync. */
  games: number;
  players: number;
  /** Rows the NBA response carried. */
  fetched: number;
  syncedAt: string | null;
  /** Set when the response was empty or short and the cache was kept. */
  warning: string | null;
}

export interface PlayerOption {
  playerId: number;
  name: string;
  team: string;
  games: number;
}

export interface TrendQuery {
  season: string;
  seasonType: string;
  playerId: number;
  stat: string;
  window: string;
  line: number;
}

export interface TrendGame {
  gameId: string;
  gameDate: string;
  matchup: string;
  opponent: string;
  location: string;
  result: string;
  minutes: number;
  stat: number;
  over: boolean;
  movingAvg: number | null;
  /** Days off before this game, counted from his previous game played. Null for his first. */
  restDays: number | null;
  points: number;
  rebounds: number;
  assists: number;
  steals: number;
  blocks: number;
  turnovers: number;
  plusMinus: number;
}

export interface TrendSummary {
  sample: number;
  overs: number;
  hitRate: number | null;
  wilsonLow: number | null;
  wilsonHigh: number | null;
  mean: number | null;
  median: number | null;
  sd: number | null;
  min: number | null;
  max: number | null;
  /** 0-minute games in the window's span. They are not in the sample. */
  dnp: number;
}

/** One window against the line. Rust scores all four so the page does no hit-rate math. */
export interface TrendSplit {
  window: string;
  sample: number;
  overs: number;
  hitRate: number | null;
  wilsonLow: number | null;
  wilsonHigh: number | null;
  dnp: number;
}

export interface TrendReport {
  playerId: number;
  playerName: string;
  team: string;
  season: string;
  seasonType: string;
  stat: string;
  statLabel: string;
  window: string;
  windowLabel: string;
  line: number;
  games: TrendGame[];
  summary: TrendSummary;
  /** Last 5, last 10, last 20, and the season against the same line. */
  splits: TrendSplit[];
}

export interface BoardQuery {
  season: string;
  seasonType: string;
  stat: string;
  minGames: number;
  line: number;
}

export interface BoardSplit {
  overs: number;
  games: number;
}

export interface BoardRow {
  playerId: number;
  name: string;
  team: string;
  /** Games played. 0-minute games are left out of every split. */
  games: number;
  dnp: number;
  mean: number;
  last5: BoardSplit;
  last10: BoardSplit;
  last20: BoardSplit;
  season: BoardSplit;
}

export interface TrainQuery {
  season: string;
  seasonType: string;
}

export interface TrainStatReport {
  stat: string;
  label: string;
  error: string | null;
  trainRows: number;
  holdoutRows: number;
  holdoutMae: number | null;
  baselineMae: number | null;
  holdoutCoverage: number | null;
  fittedAt: string | null;
  priorMinutes: number | null;
  opponentMinutes: number | null;
  shiftPrior: number | null;
  homeMultiplier: number | null;
  restPerDay: number | null;
  settingsStored: boolean;
}

export interface TrainReport {
  season: string;
  seasonType: string;
  stats: TrainStatReport[];
}

export interface PredictQuery {
  season: string;
  seasonType: string;
  playerId: number;
  stat: string;
  line: number;
  opponent: string;
  home: boolean;
  restDays: number;
  minutes: number | null;
  window: string;
}

export interface Prediction {
  stat: string;
  statLabel: string;
  mean: number;
  low: number;
  high: number;
  clearProbability: number;
  sigma: number;
  pmf: number[];
  shiftProbability: number | null;
  minutes: number;
  holdoutMae: number | null;
  baselineMae: number | null;
  holdoutCoverage: number | null;
  trainRows: number;
  holdoutRows: number;
}
