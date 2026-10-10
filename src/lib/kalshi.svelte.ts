import { errorText, loadKalshi, refreshKalshi } from "./api";
import type { KalshiQuote, KalshiRefresh } from "./types";

/** The newest stored Kalshi pull, shared by the Board column and the Player ladder. */
export const kalshi = $state({
  loaded: false,
  pulling: false,
  pulledAt: null as string | null,
  rows: [] as KalshiQuote[],
  last: null as KalshiRefresh | null,
  error: null as string | null,
});

export async function loadKalshiRows(): Promise<void> {
  try {
    const result = await loadKalshi();
    kalshi.pulledAt = result.pulledAt;
    kalshi.rows = result.rows;
    kalshi.loaded = true;
  } catch (caught: unknown) {
    kalshi.error = errorText(caught);
  }
}

/** Manual refresh: one read of Kalshi's public NBA props, then reload the stored rows. */
export async function pullKalshi(season: string): Promise<void> {
  if (kalshi.pulling) return;
  kalshi.pulling = true;
  kalshi.error = null;
  try {
    kalshi.last = await refreshKalshi(season);
    await loadKalshiRows();
  } catch (caught: unknown) {
    kalshi.error = errorText(caught);
  } finally {
    kalshi.pulling = false;
  }
}
