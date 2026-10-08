import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SyncReport } from "./types";

const api = vi.hoisted(() => ({
  syncSeason: vi.fn(),
  loadBootstrap: vi.fn(),
}));

vi.mock("./api", () => ({
  inTauri: () => true,
  errorText: (error: unknown) => (typeof error === "string" ? error : "Something went wrong."),
  syncSeason: api.syncSeason,
  loadBootstrap: api.loadBootstrap,
}));

const { carrying, playersReady, session, syncCurrent } = await import("./session.svelte");

function report(extra: Partial<SyncReport>): SyncReport {
  return {
    season: "2025-26",
    seasonType: "Regular Season",
    games: 40,
    players: 2,
    fetched: 40,
    syncedAt: "2026-10-07T18:00:00Z",
    warning: null,
    rosterPlayers: null,
    playoffTeams: null,
    ...extra,
  };
}

describe("syncCurrent", () => {
  beforeEach(() => {
    api.syncSeason.mockReset();
    api.loadBootstrap.mockReset();
    api.loadBootstrap.mockResolvedValue({ seasons: [] });
    session.preview = false;
    session.notice = null;
    session.warning = null;
    session.error = null;
  });

  it("shows the cached count after a good sync", async () => {
    api.syncSeason.mockResolvedValue(report({}));
    await syncCurrent();
    expect(session.notice).toBe("Cached 40 games for 2 players.");
    expect(session.warning).toBeNull();
    expect(session.syncing).toBe(false);
  });

  it("shows the warning when the cache was kept", async () => {
    api.syncSeason.mockResolvedValue(
      report({ fetched: 0, warning: "NBA returned 0 rows for 2025-26 Regular Season. The 40 cached games were kept." }),
    );
    await syncCurrent();
    expect(session.warning).toContain("were kept");
    expect(session.notice).toBeNull();
  });

  it("adds the roster count when the roster call answered", async () => {
    api.syncSeason.mockResolvedValue(
      report({ games: 0, fetched: 0, warning: "NBA returned 0 rows for 2026-27 Regular Season. Nothing was cached.", rosterPlayers: 614 }),
    );
    await syncCurrent();
    expect(session.warning).toBe(
      "NBA returned 0 rows for 2026-27 Regular Season. Nothing was cached. Rosters list 614 players.",
    );
  });

  it("shows the command error text", async () => {
    api.syncSeason.mockRejectedValue("A sync is already running.");
    await syncCurrent();
    expect(session.error).toBe("A sync is already running.");
    expect(session.syncing).toBe(false);
  });
});

describe("opening night", () => {
  const status = (season: string, seasonType: string, games: number) => ({
    season,
    seasonType,
    games,
    players: games ? 500 : 0,
    firstGame: null,
    lastGame: null,
    syncedAt: null,
  });

  beforeEach(() => {
    session.season = "2026-27";
    session.seasonType = "Regular Season";
    session.seed = true;
    session.seasons = [status("2025-26", "Regular Season", 26000), status("2026-27", "Regular Season", 0)];
  });

  it("lists players before the first game when last season is cached and carry is on", () => {
    expect(carrying()).toBe(true);
    expect(playersReady()).toBe(true);
  });

  it("waits for a sync when carry is off or last season is missing", () => {
    session.seed = false;
    expect(playersReady()).toBe(false);
    session.seed = true;
    session.seasons = [status("2026-27", "Regular Season", 0)];
    expect(carrying()).toBe(false);
    expect(playersReady()).toBe(false);
  });

  it("playoffs carry their own regular season", () => {
    session.seasonType = "Playoffs";
    session.seasons = [status("2026-27", "Regular Season", 1200), status("2026-27", "Playoffs", 0)];
    expect(playersReady()).toBe(true);
  });
});
