import { describe, expect, it } from "vitest";
import fixture from "../../tests/fixtures/desk-math.json";
import { atLeast, mean, median, movingAverage, sampleSd, wilson } from "./deskMath";

function close(actual: number | null | undefined, expected: number | null, label: string) {
  if (expected == null) {
    expect(actual ?? null, label).toBeNull();
    return;
  }
  expect(actual, label).not.toBeNull();
  expect(Math.abs((actual as number) - expected), `${label}: ${actual} vs ${expected}`).toBeLessThan(1e-9);
}

/** cargo test reads the same file in stats.rs and engine/bayes.rs. */
describe("the shared fixture", () => {
  it("matches wilson", () => {
    for (const item of fixture.wilson) {
      const band = wilson(item.overs, item.games);
      close(band?.low ?? null, item.expected?.[0] ?? null, `wilson ${item.overs}/${item.games} low`);
      close(band?.high ?? null, item.expected?.[1] ?? null, `wilson ${item.overs}/${item.games} high`);
    }
  });

  it("matches median, mean, and the sample deviation", () => {
    for (const item of fixture.median) close(median(item.values), item.expected, "median");
    for (const item of fixture.mean) close(mean(item.values), item.expected, "mean");
    for (const item of fixture.sampleSd) close(sampleSd(item.values), item.expected, "sample sd");
  });

  it("matches the moving average", () => {
    for (const item of fixture.movingAverage) {
      const averages = movingAverage(item.values, item.width);
      expect(averages).toHaveLength(item.expected.length);
      averages.forEach((value, index) => close(value, item.expected[index], "moving average"));
    }
  });

  it("matches the tail sum at whole, half, and almost-whole lines", () => {
    for (const item of fixture.atLeast.cases) {
      close(atLeast(fixture.atLeast.pmf, item.line), item.expected, `atLeast ${item.line}`);
    }
    expect(atLeast(fixture.atLeastEmpty.pmf, fixture.atLeastEmpty.line)).toBe(fixture.atLeastEmpty.expected);
  });
});

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
