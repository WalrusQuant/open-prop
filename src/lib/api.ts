import {
  previewBoard,
  previewBootstrap,
  previewPlayers,
  previewTrend,
} from "./preview";
import type {
  BoardQuery,
  KalshiQuotes,
  KalshiRefresh,
  BoardRow,
  Bootstrap,
  PlayerOption,
  PredictQuery,
  Prediction,
  SyncReport,
  TrainQuery,
  TrainReport,
  TrainStatReport,
  TrendQuery,
  TrendReport,
} from "./types";

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(name, args);
}

export async function loadBootstrap(): Promise<Bootstrap> {
  if (!inTauri()) return previewBootstrap();
  return command<Bootstrap>("bootstrap");
}

export async function syncSeason(season: string, seasonType: string): Promise<SyncReport> {
  return command<SyncReport>("sync_season", { season, seasonType });
}

/** With `carry`, players from the carried season and this season's rosters are listed before their first game. */
export async function loadPlayers(season: string, seasonType: string, carry = false): Promise<PlayerOption[]> {
  if (!inTauri()) return previewPlayers(season, seasonType, carry);
  return command<PlayerOption[]>("players", { season, seasonType, carry });
}

export async function loadTrend(query: TrendQuery): Promise<TrendReport> {
  if (!inTauri()) return previewTrend(query);
  return command<TrendReport>("trend", { query });
}

export async function loadBoard(query: BoardQuery): Promise<BoardRow[]> {
  if (!inTauri()) return previewBoard(query);
  return command<BoardRow[]>("leaderboard", { query });
}

export async function trainModels(query: TrainQuery): Promise<TrainReport> {
  if (!inTauri()) throw new Error("Train runs in the desktop app.");
  return command<TrainReport>("train_models", { query });
}

export async function loadModelScores(query: TrainQuery): Promise<TrainStatReport[]> {
  if (!inTauri()) return [];
  return command<TrainStatReport[]>("model_scores", { query });
}

export async function loadPrediction(query: PredictQuery): Promise<Prediction> {
  if (!inTauri()) throw new Error("Train runs in the desktop app.");
  return command<Prediction>("predict", { query });
}

/** Reads Kalshi's open NBA props. Public data, no key, nothing is traded. */
export async function refreshKalshi(season: string): Promise<KalshiRefresh> {
  if (!inTauri()) throw new Error("Kalshi prices load in the desktop app.");
  return command<KalshiRefresh>("kalshi_refresh", { season });
}

export async function loadKalshi(stat: string | null = null, playerId: number | null = null): Promise<KalshiQuotes> {
  if (!inTauri()) return { pulledAt: null, rows: [] };
  return command<KalshiQuotes>("kalshi_quotes", { query: { stat, playerId } });
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
