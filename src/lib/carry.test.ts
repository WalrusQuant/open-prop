import { describe, expect, it } from "vitest";
import { gamesLabel, lastSeasonSentence, teamNote } from "./carry";

describe("gamesLabel", () => {
  const base = { games: 0, teamSource: "season" as const, rookie: false };

  it("marks a player with no games this season", () => {
    expect(gamesLabel({ ...base, games: 0 })).toBe("no games yet");
    expect(gamesLabel({ ...base, games: 1 })).toBe("1 game");
    expect(gamesLabel({ ...base, games: 12 })).toBe("12 games");
  });

  it("labels a waived player and a rookie", () => {
    expect(gamesLabel({ ...base, games: 12, teamSource: "offRoster" })).toBe("Not on a roster");
    expect(gamesLabel({ ...base, games: 0, teamSource: "roster", rookie: true })).toBe(
      "Rookie, needs 5 games",
    );
  });
});

describe("teamNote", () => {
  it("names where a team came from", () => {
    expect(teamNote("season", "2025-26 Regular Season")).toBeNull();
    expect(teamNote("roster", "2025-26 Regular Season")).toBe("Team from this season's roster");
    expect(teamNote("offRoster", "2025-26 Regular Season")).toBe("Not on a roster");
    expect(teamNote("lastSeason", "2025-26 Regular Season")).toBe("Team from 2025-26");
    expect(teamNote("lastSeason", null)).toBe("Team from last season");
  });
});

describe("lastSeasonSentence", () => {
  const split = { window: "season", sample: 70, overs: 41, hitRate: 41 / 70, wilsonLow: 0.47, wilsonHigh: 0.7, dnp: 0 };

  it("labels the rate as last season", () => {
    expect(lastSeasonSentence({ season: "2025-26 Regular Season", split, median: 22 }, 20.5)).toBe(
      "Last season (2025-26): 41 of 70 games were 20.5 or more, 59%.",
    );
  });

  it("says so when there is nothing to compare", () => {
    const empty = { ...split, sample: 0, overs: 0, hitRate: null };
    expect(lastSeasonSentence({ season: "2025-26 Playoffs", split: empty, median: null }, 20.5)).toBe(
      "No 2025-26 Playoffs games to compare.",
    );
  });
});
