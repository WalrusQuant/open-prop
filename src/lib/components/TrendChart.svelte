<script lang="ts">
  import * as d3 from "d3";
  import { shortDate } from "$lib/format";
  import { theme } from "$lib/theme.svelte";
  import type { TrendGame } from "$lib/types";

  let {
    games,
    line,
    label,
  }: {
    games: TrendGame[];
    line: number;
    label: string;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let tip = $state<{ x: number; y: number; game: TrendGame } | null>(null);

  function color(name: string, fallback: string): string {
    const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return value || fallback;
  }

  function draw() {
    if (!host || games.length === 0) return;
    const width = host.clientWidth;
    if (width < 80) return;
    const height = 260;
    const margin = { top: 18, right: 12, bottom: games.length > 16 ? 72 : 48, left: 42 };
    const svg = d3.select(host).select<SVGSVGElement>("svg");
    svg.attr("viewBox", `0 0 ${width} ${height}`).attr("width", width).attr("height", height);
    svg.selectAll("*").remove();

    const innerWidth = width - margin.left - margin.right;
    const innerHeight = height - margin.top - margin.bottom;
    const root = svg.append("g").attr("transform", `translate(${margin.left},${margin.top})`);
    const ink = color("--muted", "#93a0ac");
    const rule = color("--rule", "#2a323b");
    const over = color("--over", "#3dce7a");
    const under = color("--under", "#ff6b78");
    const lineColor = color("--line", "#f4f7fa");
    const average = color("--avg", "#f0c14b");

    const peak = d3.max(games, (game) => Math.max(game.stat, game.movingAvg ?? 0, line)) ?? 1;
    const y = d3
      .scaleLinear()
      .domain([0, peak * 1.12 || 1])
      .nice()
      .range([innerHeight, 0]);
    const x = d3
      .scaleBand<string>()
      .domain(games.map((game) => game.gameId))
      .range([0, innerWidth])
      .padding(0.24);
    const step = Math.max(1, Math.ceil(games.length / 8));
    const ticks = games.filter((_, index) => index % step === 0).map((game) => game.gameId);

    root
      .append("g")
      .call(d3.axisLeft(y).ticks(5).tickSize(-innerWidth))
      .call((group) => {
        group.selectAll("text").attr("fill", ink);
        group.selectAll("line").attr("stroke", rule);
        group.select(".domain").attr("stroke", "none");
      });

    root
      .append("g")
      .attr("transform", `translate(0,${innerHeight})`)
      .call(
        d3
          .axisBottom(x)
          .tickValues(ticks)
          .tickFormat((id) => {
            const game = games.find((item) => item.gameId === id);
            return game ? shortDate(game.gameDate) : "";
          }),
      )
      .call((group) => {
        group.selectAll("text").attr("fill", ink).attr("transform", games.length > 16 ? "rotate(-38)" : null).style("text-anchor", games.length > 16 ? "end" : "middle");
        group.selectAll("line, path").attr("stroke", rule);
      });

    root
      .append("line")
      .attr("x1", 0)
      .attr("x2", innerWidth)
      .attr("y1", y(line))
      .attr("y2", y(line))
      .attr("stroke", lineColor)
      .attr("stroke-dasharray", "4 4")
      .attr("stroke-width", 1.6);
    root
      .append("text")
      .attr("x", innerWidth)
      .attr("y", Math.max(12, y(line) - 6))
      .attr("text-anchor", "end")
      .attr("fill", lineColor)
      .text(String(line));

    root
      .selectAll("rect")
      .data(games)
      .join("rect")
      .attr("x", (game) => x(game.gameId) ?? 0)
      .attr("y", (game) => y(Math.max(game.stat, 0)))
      .attr("width", x.bandwidth())
      .attr("height", (game) => Math.max(0, innerHeight - y(Math.max(game.stat, 0))))
      .attr("fill", (game) => (game.over ? over : under))
      .on("mousemove", (event: MouseEvent, game: TrendGame) => {
        const bounds = host?.getBoundingClientRect();
        if (!bounds) return;
        tip = {
          x: event.clientX - bounds.left + 14,
          y: event.clientY - bounds.top - 8,
          game,
        };
      })
      .on("mouseleave", () => {
        tip = null;
      });

    const series = games.filter((game) => game.movingAvg != null);
    const path = d3
      .line<TrendGame>()
      .x((game) => (x(game.gameId) ?? 0) + x.bandwidth() / 2)
      .y((game) => y(game.movingAvg ?? 0));
    root
      .append("path")
      .attr("d", path(series) ?? "")
      .attr("fill", "none")
      .attr("stroke", average)
      .attr("stroke-width", 2.25)
      .attr("stroke-linejoin", "round")
      .attr("stroke-linecap", "round");
  }

  $effect(() => {
    theme.mode;
    games;
    line;
    label;
    if (!host) return;
    draw();
    const observer = new ResizeObserver(() => draw());
    observer.observe(host);
    return () => observer.disconnect();
  });
</script>

<div class="chart" bind:this={host}>
  <svg role="img" aria-label={label}></svg>
  {#if tip}
    <div class="tip" style:left="{tip.x}px" style:top="{tip.y}px">
      <strong>{shortDate(tip.game.gameDate)} · {tip.game.opponent}</strong>
      <span>{tip.game.stat} · {tip.game.over ? "over" : "under"}</span>
      {#if tip.game.movingAvg != null}
        <span>3-game avg {tip.game.movingAvg.toFixed(1)}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .chart {
    position: relative;
    min-height: 260px;
  }

  svg {
    display: block;
    width: 100%;
    overflow: visible;
  }

  .chart :global(text) {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .tip {
    position: absolute;
    z-index: 2;
    display: grid;
    gap: 0.15rem;
    min-width: 9rem;
    padding: 0.45rem 0.55rem;
    background: var(--ink);
    color: var(--bg);
    font-family: var(--font-mono);
    font-size: 0.75rem;
    pointer-events: none;
  }
</style>
