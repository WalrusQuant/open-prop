/**
 * The stat and window catalog for the screens. Rust `models.rs` owns the ids and labels, and
 * a Rust test checks `catalog.json` against `Stat::all()` and `Window::all()`, so adding a stat
 * means one enum arm, one spec file, and one entry here.
 */
import catalog from "./catalog.json";
import type { CatalogItem } from "./types";

export interface StatEntry extends CatalogItem {
  short: string;
  defaultLine: number;
  /** Plain-language parts for a combo stat. */
  parts?: string;
}

export interface WindowEntry extends CatalogItem {
  short: string;
}

export const STATS: readonly StatEntry[] = catalog.stats;
export const WINDOWS: readonly WindowEntry[] = catalog.windows;
export const STAT_IDS: readonly string[] = STATS.map((item) => item.id);

const byStat = new Map(STATS.map((item) => [item.id, item]));
const byWindow = new Map(WINDOWS.map((item) => [item.id, item]));
const chipRank = new Map(catalog.chipOrder.map((id, index) => [id, index]));

export function statShort(id: string, fallback = id): string {
  return byStat.get(id)?.short ?? fallback;
}

export function windowShort(id: string, fallback = id): string {
  return byWindow.get(id)?.short ?? fallback;
}

export function defaultLine(id: string): number {
  return byStat.get(id)?.defaultLine ?? 10;
}

export function statParts(id: string): string | null {
  return byStat.get(id)?.parts ?? null;
}

/** The order of the stat chips on the player page. Unknown ids go last. */
export function chipOrder<T extends { id: string }>(items: readonly T[]): T[] {
  const rank = (id: string) => chipRank.get(id) ?? Number.MAX_SAFE_INTEGER;
  return [...items].sort((left, right) => rank(left.id) - rank(right.id));
}
