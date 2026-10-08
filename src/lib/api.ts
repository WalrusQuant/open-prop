import {
  previewBoard,
  previewBootstrap,
  previewPlayers,
  previewTrend,
} from "./preview";
import type {
  BoardQuery,
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

export async function loadPlayers(season: string, seasonType: string): Promise<PlayerOption[]> {
  if (!inTauri()) return previewPlayers();
  return command<PlayerOption[]>("players", { season, seasonType });
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

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
