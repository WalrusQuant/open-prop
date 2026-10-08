import { describe, expect, it } from "vitest";
import { atLeast, median, movingAverage, sampleSd, wilson } from "./deskMath";

describe("desk math", () => {
  it("puts seven of ten at about 40% to 89%", () => {
    const band = wilson(7, 10);
    expect(band?.low).toBeCloseTo(0.3968, 3);
    expect(band?.high).toBeCloseTo(0.8922, 3);
    expect(wilson(0, 0)).toBeNull();
  });

  it("waits three games for the moving average", () => {
    const average = movingAverage([1, 2, 3, 6], 3);
    expect(average[0]).toBeNull();
    expect(average[1]).toBeNull();
    expect(average[2]).toBeCloseTo(2, 9);
    expect(average[3]).toBeCloseTo(11 / 3, 9);
  });

  it("takes the even median and the sample deviation", () => {
    expect(median([1, 2, 3, 4])).toBe(2.5);
    expect(sampleSd([5])).toBeNull();
    expect(sampleSd([2, 4])).toBeCloseTo(Math.sqrt(2), 9);
  });

  it("sums the tail from the line up", () => {
    const pmf = [0.2, 0.2, 0.2, 0.2, 0.2];
    expect(atLeast(pmf, 0)).toBeCloseTo(1, 12);
    expect(atLeast(pmf, 2)).toBeCloseTo(0.6, 12);
    expect(atLeast(pmf, 1.5)).toBeCloseTo(0.6, 12);
    expect(atLeast([], 3)).toBe(0);
  });
});
