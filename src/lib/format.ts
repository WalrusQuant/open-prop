import type { TrendReport } from "./types";

export function round1(value: number): number {
  return Math.round(value * 10) / 10;
}

export function formatLine(value: number): string {
  const rounded = round1(value);
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

export function formatStat(value: number | null, digits = 1): string {
  if (value == null || Number.isNaN(value)) return "—";
  return value.toFixed(digits);
}

export function formatPct(value: number | null): string {
  if (value == null || Number.isNaN(value)) return "—";
  return `${(value * 100).toFixed(1)}%`;
}

export function shortDate(iso: string): string {
  const [year, month, day] = iso.split("-").map(Number);
  if (!year || !month || !day) return iso;
  return new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" }).format(
    new Date(year, month - 1, day),
  );
}

export function formatWhen(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

export function hitSentence(report: TrendReport): string {
  const { sample, overs } = report.summary;
  if (sample === 0) return "No games in this window.";
  const unit = report.statLabel.toLowerCase();
  return `${overs} of ${sample} games reached ${formatLine(report.line)} ${unit}.`;
}

/** 0-minute games are left out of every hit rate. Empty when there are none. */
export function dnpSentence(count: number): string {
  if (!Number.isFinite(count) || count <= 0) return "";
  return count === 1
    ? "1 game at 0 minutes (DNP) is left out."
    : `${count} games at 0 minutes (DNP) are left out.`;
}

export function intervalSentence(report: TrendReport): string {
  const { sample, wilsonLow, wilsonHigh } = report.summary;
  if (sample === 0 || wilsonLow == null || wilsonHigh == null) {
    return "There is no hit rate to estimate.";
  }
  return `The 95% Wilson interval runs from ${formatPct(wilsonLow)} to ${formatPct(wilsonHigh)}.`;
}

export function sampleNote(sample: number): string {
  if (sample === 0) return "Sync a season to fill the cache.";
  if (sample < 30) {
    return `${sample} games is a small sample. The width of the interval is the result.`;
  }
  return "The count in the middle is the center of an interval, not a promise about the next game.";
}

export function csvFor(report: TrendReport): string {
  const header = [
    "date",
    "opponent",
    "location",
    "result",
    "minutes",
    "stat",
    "stat_name",
    "line",
    "over",
    "moving_avg_3",
    "points",
    "rebounds",
    "assists",
    "steals",
    "blocks",
    "turnovers",
    "plus_minus",
  ];
  const lines = [...report.games].reverse().map((game) =>
    [
      game.gameDate,
      game.opponent,
      game.location,
      game.result,
      game.minutes.toFixed(1),
      game.stat,
      report.stat,
      report.line,
      game.over ? "over" : "under",
      game.movingAvg == null ? "" : game.movingAvg.toFixed(2),
      game.points,
      game.rebounds,
      game.assists,
      game.steals,
      game.blocks,
      game.turnovers,
      game.plusMinus,
    ]
      .map(csvCell)
      .join(","),
  );
  return [header.join(","), ...lines].join("\n");
}

function csvCell(value: string | number): string {
  const text = String(value);
  return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

export function downloadCsv(report: TrendReport) {
  const blob = new Blob([csvFor(report)], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  const slug = report.playerName.toLowerCase().replace(/[^a-z0-9]+/g, "-");
  link.href = url;
  link.download = `open-prop-${slug}-${report.stat}-${report.window}.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
