import { describe, expect, it } from "vitest";
import { cents, kalshiLists, ladder, noEdge, rungAt, signedPoints, thinReason, yesEdge } from "./kalshi";
import type { KalshiQuote } from "./types";

function quote(threshold: number, bid: number | null, ask: number | null, extra: Partial<KalshiQuote> = {}): KalshiQuote {
  const mid = bid !== null && ask !== null ? (bid + ask) / 2 : null;
  const spread = bid !== null && ask !== null ? ask - bid : null;
  return {
    marketTicker: `KXNBAPTS-26OCT08BOSCLE-BOSJTATUM0-${threshold}`,
    eventTicker: "KXNBAPTS-26OCT08BOSCLE",
    stat: "points",
    playerName: "Jayson Tatum",
    team: "BOS",
    playerId: 1628369,
    threshold,
    yesBid: bid,
    yesAsk: ask,
    mid,
    spread,
    volume: 500,
    openInterest: 100,
    thin: spread === null || spread > 0.1,
    gameTime: "2026-10-09T02:00:00Z",
    ...extra,
  };
}

describe("kalshi", () => {
  it("lists only stats with a series", () => {
    expect(kalshiLists("points")).toBe(true);
    expect(kalshiLists("points_assists_rebounds")).toBe(true);
    expect(kalshiLists("turnovers")).toBe(false);
    expect(kalshiLists("field_goals_made")).toBe(false);
    expect(kalshiLists("field_goals_attempted")).toBe(false);
  });

  it("edge is model minus the price paid", () => {
    expect(yesEdge(0.7, 0.62)).toBeCloseTo(0.08);
    expect(yesEdge(0.7, null)).toBeNull();
    expect(noEdge(0.7, 0.61)).toBeCloseTo(-0.09);
  });

  it("ladder reads the model tail at each rung", () => {
    // Model mass: 0..29 with all weight on 22, so P(>=20) = 1 and P(>=25) = 0.
    const pmf = Array.from({ length: 30 }, (_, index) => (index === 22 ? 1 : 0));
    const rows = [quote(25, 0.3, 0.35), quote(20, 0.7, 0.72), quote(15, null, null, { playerId: 1 })];
    const result = ladder(rows, 1628369, "points", pmf);
    expect(result.map((row) => row.quote.threshold)).toEqual([20, 25]);
    expect(result[0].model).toBeCloseTo(1);
    expect(result[0].edge).toBeCloseTo(0.28);
    expect(result[1].edge).toBeCloseTo(-0.35);
    expect(ladder(rows, 1628369, "points", null)[0].edge).toBeNull();
  });

  it("finds the rung at the board line", () => {
    const rows = [quote(20, 0.7, 0.72), quote(25, 0.3, 0.35)];
    expect(rungAt(rows, 1628369, "points", 25)?.yesAsk).toBe(0.35);
    expect(rungAt(rows, 1628369, "points", 24.5)).toBeNull();
    expect(rungAt(rows, 1628369, "rebounds", 25)).toBeNull();
  });

  it("formats and explains thin rungs", () => {
    expect(cents(0.615)).toBe("62¢");
    expect(cents(null)).toBe("—");
    expect(signedPoints(0.08)).toBe("+8.0");
    expect(signedPoints(-0.035)).toBe("-3.5");
    expect(thinReason(quote(25, null, 0.4))).toMatch(/empty/);
    expect(thinReason(quote(25, 0.2, 0.4))).toMatch(/wider/);
    expect(thinReason(quote(25, 0.3, 0.32, { thin: true }))).toMatch(/100 contracts/);
    expect(thinReason(quote(25, 0.3, 0.32))).toBeNull();
  });
});
