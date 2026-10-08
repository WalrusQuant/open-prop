/**
 * Same definitions as the Rust desk (`stats.rs`, `engine/bayes.rs`).
 *
 * The desktop app gets the splits, Wilson bands, averages, and medians from Rust. These copies
 * only feed the browser preview's invented backend in `preview.ts`. `atLeast` is the exception:
 * the player page sums the model's tail with it so editing the number does not wait on a round
 * trip. `tests/fixtures/desk-math.json` is checked by both `cargo test` and vitest, so the two
 * languages cannot drift.
 */

export function movingAverage(values: number[], width = 3): Array<number | null> {
  return values.map((_, index) => {
    if (width === 0 || index + 1 < width) return null;
    const slice = values.slice(index + 1 - width, index + 1);
    return slice.reduce((sum, value) => sum + value, 0) / width;
  });
}

export function mean(values: number[]): number | null {
  if (values.length === 0) return null;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

export function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
}

export function sampleSd(values: number[]): number | null {
  if (values.length < 2) return null;
  const center = mean(values);
  if (center == null) return null;
  const square = values.reduce((sum, value) => sum + (value - center) ** 2, 0);
  return Math.sqrt(square / (values.length - 1));
}

export function wilson(successes: number, trials: number): { low: number; high: number } | null {
  if (trials === 0) return null;
  const n = trials;
  const phat = successes / n;
  const z = 1.96;
  const z2 = z * z;
  const denom = 1 + z2 / n;
  const center = (phat + z2 / (2 * n)) / denom;
  const margin = (z * Math.sqrt((phat * (1 - phat)) / n + z2 / (4 * n * n))) / denom;
  return {
    low: Math.min(1, Math.max(0, center - margin)),
    high: Math.min(1, Math.max(0, center + margin)),
  };
}

/** P(Y >= line) for a probability mass on 0, 1, 2, …. An integer line starts there. 12.5 starts at 13. */
export function atLeast(pmf: number[], line: number): number {
  if (pmf.length === 0 || !Number.isFinite(line)) return 0;
  const start =
    Math.abs(line - Math.floor(line)) < 1e-9 ? Math.max(0, Math.floor(line)) : Math.ceil(Math.max(0, line));
  let total = 0;
  for (let index = start; index < pmf.length; index += 1) total += pmf[index] ?? 0;
  return Math.min(1, Math.max(0, total));
}
