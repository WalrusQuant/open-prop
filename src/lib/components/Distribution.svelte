<script lang="ts">
  import { formatLine } from "$lib/format";
  import type { TrendGame } from "$lib/types";

  let {
    games,
    line,
    trendMean,
    windowLabel,
    statLabel,
    mean,
    pmf = null,
  }: {
    games: TrendGame[];
    line: number;
    trendMean: number | null;
    windowLabel: string;
    statLabel: string;
    mean: number | null;
    pmf?: number[] | null;
  } = $props();

  const WIDTH = 640;
  const HEIGHT = 176;
  const PAD_L = 8;
  const PAD_R = 8;
  const PAD_T = 14;
  const PAD_B = 24;

  let picture = $derived.by(() => {
    const values = games.map((game) => game.stat).filter((value) => Number.isFinite(value));
    const marks = [line, trendMean, mean].filter(
      (value): value is number => value != null && Number.isFinite(value),
    );
    const masses = pmf ?? [];
    let support = 0;
    for (let index = masses.length - 1; index >= 0; index -= 1) {
      if ((masses[index] ?? 0) > 0.002) {
        support = index;
        break;
      }
    }
    if (values.length === 0 && marks.length === 0 && support === 0) return null;
    const lo = 0;
    const top = Math.max(1, ...values, ...marks, support);
    let hi = top + Math.max(1, top * 0.08);
    if (!(hi > lo)) hi = lo + 1;
    const span = hi - lo;
    const binCount = span > 40 ? 18 : span > 16 ? 12 : Math.max(6, Math.round(span));
    const binWidth = span / binCount;
    const counts = Array.from({ length: binCount }, () => 0);
    for (const value of values) {
      let index = Math.floor((value - lo) / binWidth);
      if (index < 0) index = 0;
      if (index >= binCount) index = binCount - 1;
      counts[index] += 1;
    }
    const countPeak = Math.max(1, ...counts);
    const massPeak = Math.max(0, ...masses);
    const curve =
      massPeak > 0
        ? Array.from({ length: Math.floor(hi) + 1 }, (_, index) => ({
            x: index,
            y: ((masses[index] ?? 0) / massPeak) * countPeak,
          }))
        : [];
    const peak = Math.max(countPeak, ...curve.map((point) => point.y));
    const plotW = WIDTH - PAD_L - PAD_R;
    const plotH = HEIGHT - PAD_T - PAD_B;
    const xOf = (value: number) => PAD_L + ((value - lo) / span) * plotW;
    const yOf = (count: number) => PAD_T + plotH - (count / peak) * plotH;
    const bars = counts.map((count, index) => {
      const start = lo + index * binWidth;
      const x = xOf(start);
      return {
        x,
        y: yOf(count),
        w: Math.max(0.8, xOf(start + binWidth) - x - 1.4),
        h: yOf(0) - yOf(count),
      };
    });
    const path = curve
      .map(
        (point, index) =>
          `${index === 0 ? "M" : "L"} ${xOf(point.x).toFixed(1)} ${yOf(point.y).toFixed(1)}`,
      )
      .join(" ");
    const at = (value: number | null) =>
      value != null && Number.isFinite(value) ? xOf(value) : null;
    return {
      bars,
      path,
      base: yOf(0),
      projectionX: at(mean),
      trendX: at(trendMean),
      lineX: at(line),
      ticks: [lo, (lo + hi) / 2, hi].map((value) => ({
        x: xOf(value),
        label: formatLine(value),
      })),
    };
  });

  let gap = $derived.by(() => {
    if (mean == null || trendMean == null) return null;
    const delta = mean - trendMean;
    if (Math.abs(delta) < 0.05) return "The model matches the average of these games.";
    const body = Math.abs(delta).toFixed(1);
    return delta > 0
      ? `The model is ${body} above the average of these games.`
      : `The model is ${body} below the average of these games.`;
  });
</script>

<section class="panel">
  <h2>The model next to {windowLabel.toLowerCase()}</h2>
  {#if picture}
    <svg
      class="dist"
      viewBox="0 0 {WIDTH} {HEIGHT}"
      role="img"
      aria-label="{statLabel}. Bars are {windowLabel}. The curve is the model's distribution."
    >
      {#each picture.bars as bar, index (index)}
        <rect class="bin" x={bar.x} y={bar.y} width={bar.w} height={bar.h}></rect>
      {/each}
      {#if picture.path}
        <path d={picture.path}></path>
      {/if}
      {#if picture.trendX != null}
        <line class="trend" x1={picture.trendX} x2={picture.trendX} y1="8" y2={picture.base}></line>
      {/if}
      {#if picture.projectionX != null}
        <line class="mean" x1={picture.projectionX} x2={picture.projectionX} y1="8" y2={picture.base}></line>
      {/if}
      {#if picture.lineX != null}
        <line class="mark" x1={picture.lineX} x2={picture.lineX} y1="8" y2={picture.base}></line>
      {/if}
      {#each picture.ticks as tick, index (index)}
        <text x={tick.x} y={HEIGHT - 6} text-anchor={index === 0 ? "start" : index === 2 ? "end" : "middle"}>
          {tick.label}
        </text>
      {/each}
    </svg>
  {/if}
  <p class="legend">
    <span class="key"><i class="swatch bin"></i> {windowLabel}</span>
    <span class="key"><i class="swatch curve"></i> Model</span>
    <span class="key"><i class="swatch avg"></i> Average of these games</span>
    <span class="key"><i class="swatch line"></i> At least {formatLine(line)}</span>
  </p>
  <p class="proof">
    {#if gap}
      {gap}
    {:else}
      The curve shows in the desktop app after this stat is trained.
    {/if}
  </p>
</section>
