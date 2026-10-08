import { errorText, inTauri, loadBootstrap, syncSeason } from "./api";
import { seedSource } from "./season";
import type { CatalogItem, SeasonStatus } from "./types";

export const session = $state({
  ready: false,
  preview: false,
  syncing: false,
  /** Lives here, not on Home, so leaving the page does not re-enable Refit mid-fit. */
  training: false,
  /** Carry last season into the next refit. Kept here so it survives a page change. */
  seed: true,
  error: null as string | null,
  notice: null as string | null,
  warning: null as string | null,
  season: "2025-26",
  seasonType: "Regular Season",
  seasons: [] as SeasonStatus[],
  stats: [] as CatalogItem[],
  windows: [] as CatalogItem[],
});

export function seasonChoices(): string[] {
  return [...new Set(session.seasons.map((item) => item.season))];
}

export function currentStatus(): SeasonStatus | undefined {
  return session.seasons.find(
    (item) => item.season === session.season && item.seasonType === session.seasonType,
  );
}

/** The season the current pick carries, when it is cached. */
export function cachedSeed(): SeasonStatus | undefined {
  const source = seedSource(session.season, session.seasonType);
  if (!source) return undefined;
  return session.seasons.find(
    (item) => item.season === source.season && item.seasonType === source.seasonType && item.games > 0,
  );
}

/** Carry is on and has a season to carry, so players are listed before their first game. */
export function carrying(): boolean {
  return session.seed && cachedSeed() != null;
}

/** Players can be listed: this season has games, or carry lists last season's players. */
export function playersReady(): boolean {
  return (currentStatus()?.games ?? 0) > 0 || carrying();
}

export async function openDesk() {
  session.preview = !inTauri();
  session.error = null;
  const data = await loadBootstrap();
  session.season = data.suggestedSeason;
  session.seasonType = data.suggestedSeasonType;
  session.seasons = data.seasons;
  session.stats = data.stats;
  session.windows = data.windows;
  session.ready = true;
}

export async function syncCurrent() {
  if (session.preview) {
    session.notice = "Preview only. Sync runs in the installed app.";
    return;
  }
  session.syncing = true;
  session.error = null;
  session.notice = null;
  session.warning = null;
  try {
    const report = await syncSeason(session.season, session.seasonType);
    const roster =
      report.rosterPlayers != null ? ` Rosters list ${report.rosterPlayers.toLocaleString()} players.` : "";
    const playoffs =
      report.playoffTeams != null
        ? ` Playoff field lists ${report.playoffTeams.toLocaleString()} teams.`
        : "";
    if (report.warning) {
      session.warning = `${report.warning}${roster}${playoffs}`;
    } else {
      session.notice = `Cached ${report.games.toLocaleString()} games for ${report.players.toLocaleString()} players.${roster}${playoffs}`;
    }
    const data = await loadBootstrap();
    session.seasons = data.seasons;
  } catch (caught) {
    session.error = errorText(caught);
  } finally {
    session.syncing = false;
  }
}
