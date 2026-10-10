import { atLeast } from "./deskMath";
import type { KalshiQuote } from "./types";

/** Stats Kalshi lists a series for. TOV, FGM, and FGA have none, so they always read "no market". */
export const KALSHI_STATS = new Set([
  "points",
  "rebounds",
  "assists",
  "three_point_field_goals_made",
  "steals",
  "blocks",
  "free_throws_made",
  "points_assists_rebounds",
  "points_assists",
  "points_rebounds",
  "assists_rebounds",
]);

export function kalshiLists(stat: string): boolean {
  return KALSHI_STATS.has(stat);
}

/** Edge for yes: the model's P(stat >= threshold) minus the yes ask. Null when nobody offers. */
export function yesEdge(modelProbability: number, yesAsk: number | null): number | null {
  return yesAsk === null ? null : modelProbability - yesAsk;
}

/** Edge for no: P(stat < threshold) minus the no ask, which is one minus the yes bid. */
export function noEdge(modelProbability: number, yesBid: number | null): number | null {
  return yesBid === null ? null : 1 - modelProbability - (1 - yesBid);
}

export interface LadderRow {
  quote: KalshiQuote;
  model: number | null;
  edge: number | null;
  noEdge: number | null;
}

/** A player's rungs for one stat, lowest threshold first, with the model's tail at each rung. */
export function ladder(rows: KalshiQuote[], playerId: number, stat: string, pmf: number[] | null): LadderRow[] {
  return rows
    .filter((row) => row.playerId === playerId && row.stat === stat)
    .sort((left, right) => left.threshold - right.threshold)
    .map((quote) => {
      const model = pmf && pmf.length ? atLeast(pmf, quote.threshold) : null;
      return {
        quote,
        model,
        edge: model === null ? null : yesEdge(model, quote.yesAsk),
        noEdge: model === null ? null : noEdge(model, quote.yesBid),
      };
    });
}

/** The rung that prices exactly "line or more" for this player, if Kalshi lists one. */
export function rungAt(rows: KalshiQuote[], playerId: number, stat: string, line: number): KalshiQuote | null {
  return rows.find((row) => row.playerId === playerId && row.stat === stat && Math.abs(row.threshold - line) < 1e-9) ?? null;
}

export function cents(value: number | null): string {
  return value === null ? "—" : `${Math.round(value * 100)}¢`;
}

export function signedPoints(value: number | null): string {
  if (value === null) return "—";
  const points = Math.round(value * 1000) / 10;
  return `${points > 0 ? "+" : ""}${points.toFixed(1)}`;
}

/** Why a rung is greyed out, in a sentence. */
export function thinReason(quote: KalshiQuote): string | null {
  if (!quote.thin) return null;
  if (quote.yesBid === null || quote.yesAsk === null) return "One side of the book is empty.";
  if (quote.spread !== null && quote.spread > 0.1) return "The spread is wider than 10¢.";
  return "Fewer than 100 contracts traded.";
}
