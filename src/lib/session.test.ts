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

const { session, syncCurrent } = await import("./session.svelte");

function report(extra: Partial<SyncReport>): SyncReport {
  return {
    season: "2025-26",
    seasonType: "Regular Season",
    games: 40,
    players: 2,
    fetched: 40,
    syncedAt: "2026-10-07T18:00:00Z",
    warning: null,
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

  it("shows the command error text", async () => {
    api.syncSeason.mockRejectedValue("A sync is already running.");
    await syncCurrent();
    expect(session.error).toBe("A sync is already running.");
    expect(session.syncing).toBe(false);
  });
});
