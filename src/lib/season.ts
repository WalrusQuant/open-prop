export interface SeasonPick {
  season: string;
  seasonType: string;
}

const REGULAR = "Regular Season";

/** Where a fit carries its prior from. Mirrors `season::seed_source` in Rust. */
export function seedSource(season: string, seasonType: string): SeasonPick | null {
  if (seasonType === "Playoffs") return { season, seasonType: REGULAR };
  const match = /^(\d{4})-(\d{2})$/.exec(season);
  if (!match) return null;
  const start = Number(match[1]);
  if ((start + 1) % 100 !== Number(match[2])) return null;
  const previous = start - 1;
  return { season: `${previous}-${String((previous + 1) % 100).padStart(2, "0")}`, seasonType: REGULAR };
}

/** "2024-25 Regular Season" shortened to "2024-25" for the regular season. */
export function seedLabel(source: string): string {
  return source.endsWith(` ${REGULAR}`) ? source.slice(0, -REGULAR.length - 1) : source;
}
