import { errorText, inTauri, loadBootstrap, syncSeason } from "./api";
import type { CatalogItem, SeasonStatus } from "./types";

export const session = $state({
  ready: false,
  preview: false,
  syncing: false,
  error: null as string | null,
  notice: null as string | null,
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
  try {
    const report = await syncSeason(session.season, session.seasonType);
    session.notice = `Cached ${report.games.toLocaleString()} games for ${report.players.toLocaleString()} players.`;
    const data = await loadBootstrap();
    session.seasons = data.seasons;
  } catch (caught) {
    session.error = errorText(caught);
  } finally {
    session.syncing = false;
  }
}
