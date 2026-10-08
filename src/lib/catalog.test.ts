import { describe, expect, it } from "vitest";
import { STATS, WINDOWS, chipOrder, defaultLine, statParts, statShort, windowShort } from "./catalog";

describe("catalog", () => {
  it("has 14 stats and 4 windows", () => {
    expect(STATS).toHaveLength(14);
    expect(WINDOWS.map((item) => item.id)).toEqual(["last_5", "last_10", "last_20", "season"]);
  });

  it("gives short labels, default lines, and combo parts", () => {
    expect(statShort("points_assists_rebounds")).toBe("PRA");
    expect(statShort("unknown", "Unknown")).toBe("Unknown");
    expect(windowShort("last_10")).toBe("L10");
    expect(defaultLine("rebounds")).toBe(6);
    expect(defaultLine("unknown")).toBe(10);
    expect(statParts("points_rebounds")).toBe("points and rebounds");
    expect(statParts("points")).toBeNull();
  });

  it("puts the chips in prop-board order", () => {
    const ordered = chipOrder([{ id: "steals" }, { id: "unknown" }, { id: "points_assists_rebounds" }, { id: "points" }]);
    expect(ordered.map((item) => item.id)).toEqual(["points", "points_assists_rebounds", "steals", "unknown"]);
  });
});
