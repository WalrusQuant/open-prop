import type { LastSeason, PlayerOption, TeamSource } from "./types";
import { formatLine } from "./format";
import { seedLabel } from "./season";

/** The list shows a game count, a waived note, or that he has not played yet this season. */
export function gamesLabel(player: Pick<PlayerOption, "games" | "teamSource" | "rookie">): string {
  if (player.teamSource === "offRoster") return "Not on a roster";
  if (player.rookie) return "Rookie, needs 5 games";
  if (player.games === 0) return "no games yet";
  return player.games === 1 ? "1 game" : `${player.games} games`;
}

/** Says where the team came from when it is not his latest game this season. */
export function teamNote(source: TeamSource, carriedSeason: string | null): string | null {
  if (source === "roster") return "Team from this season's roster";
  if (source === "offRoster") return "Not on a roster";
  if (source === "lastSeason") {
    return carriedSeason ? `Team from ${seedLabel(carriedSeason)}` : "Team from last season";
  }
  return null;
}

/** One line for his carried season against the number, labelled with that season. */
export function lastSeasonSentence(last: LastSeason, line: number): string {
  const { overs, sample, hitRate } = last.split;
  const season = seedLabel(last.season);
  if (sample === 0 || hitRate == null) return `No ${season} games to compare.`;
  return `Last season (${season}): ${overs} of ${sample} games were ${formatLine(line)} or more, ${Math.round(
    hitRate * 100,
  )}%.`;
}
