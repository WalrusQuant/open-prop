import { STAT_IDS } from "$lib/catalog";

export function entries() {
  return STAT_IDS.map((stat) => ({ stat }));
}
