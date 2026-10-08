import { median, movingAverage, sampleSd, wilson } from "../src/lib/deskMath.ts";

function close(actual: number, expected: number, label: string) {
  if (Math.abs(actual - expected) > 0.001) {
    throw new Error(`${label}: ${actual} is not within 0.001 of ${expected}`);
  }
}

const band = wilson(7, 10);
if (!band) throw new Error("wilson(7, 10) returned nothing");
close(band.low, 0.3968, "wilson low");
close(band.high, 0.8922, "wilson high");

const average = movingAverage([1, 2, 3, 6], 3);
if (average[0] !== null || average[1] !== null) {
  throw new Error("the first two games of a 3-game average should be empty");
}
close(average[2] ?? NaN, 2, "third average");
close(average[3] ?? NaN, 11 / 3, "fourth average");

const middle = median([1, 2, 3, 4]);
close(middle ?? NaN, 2.5, "even median");
if (sampleSd([5]) !== null) throw new Error("a single game has no sample deviation");
close(sampleSd([2, 4]) ?? NaN, Math.sqrt(2), "sample sd");

console.log("desk math matches the published checks");
