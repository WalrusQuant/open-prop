import { describe, expect, it } from "vitest";
import { seedLabel, seedSource } from "./season";

describe("seedSource", () => {
  it("seeds a regular season from the one before", () => {
    expect(seedSource("2025-26", "Regular Season")).toEqual({
      season: "2024-25",
      seasonType: "Regular Season",
    });
    expect(seedSource("2000-01", "Regular Season")).toEqual({
      season: "1999-00",
      seasonType: "Regular Season",
    });
  });

  it("seeds playoffs from the same regular season", () => {
    expect(seedSource("2025-26", "Playoffs")).toEqual({
      season: "2025-26",
      seasonType: "Regular Season",
    });
  });

  it("has no seed for a label that is not a season", () => {
    expect(seedSource("2025-27", "Regular Season")).toBeNull();
    expect(seedSource("season", "Regular Season")).toBeNull();
  });
});

describe("seedLabel", () => {
  it("drops the season type only for the regular season", () => {
    expect(seedLabel("2024-25 Regular Season")).toBe("2024-25");
    expect(seedLabel("2025-26 Playoffs")).toBe("2025-26 Playoffs");
  });
});
