<script lang="ts">
  import { formatWhen } from "$lib/format";
  import { cents, kalshiLists, ladder, signedPoints, thinReason } from "$lib/kalshi";
  import { kalshi, loadKalshiRows, pullKalshi } from "$lib/kalshi.svelte";
  import { session } from "$lib/session.svelte";

  let {
    playerId,
    stat,
    statLabel,
    pmf,
  }: { playerId: number | null; stat: string; statLabel: string; pmf: number[] | null } = $props();

  let listed = $derived(kalshiLists(stat));
  let rows = $derived(playerId === null ? [] : ladder(kalshi.rows, playerId, stat, pmf));

  $effect(() => {
    if (!kalshi.loaded) void loadKalshiRows();
  });
</script>

<section class="kalshi-ladder" aria-label="Kalshi prices">
  <header>
    <p class="kicker">Kalshi</p>
    <h3>What Kalshi charges for each {statLabel.toLowerCase()} rung</h3>
    <button type="button" onclick={() => pullKalshi(session.season)} disabled={kalshi.pulling}>
      {kalshi.pulling ? "Reading…" : "Refresh"}
    </button>
  </header>
  {#if !listed}
    <p class="band">No market. Kalshi has no series for {statLabel.toLowerCase()}.</p>
  {:else if rows.length === 0}
    <p class="band">
      No market for this player{kalshi.pulledAt ? ` in the Kalshi read from ${formatWhen(kalshi.pulledAt)}` : ". Refresh to read Kalshi"}.
    </p>
  {:else}
    <p class="cuts">
      Yes pays when he gets that many or more. The model column is its chance of that. Edge is the model minus the
      yes ask, before Kalshi's fee. Mid is the market's own estimate. Grey rows are thin.
    </p>
    <table>
      <thead>
        <tr>
          <th class="left">Rung</th>
          <th>Model</th>
          <th>Bid</th>
          <th>Ask</th>
          <th>Mid</th>
          <th>Edge (yes)</th>
          <th>Edge (no)</th>
          <th>Volume</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.quote.marketTicker)}
          <tr class:thin={row.quote.thin} title={thinReason(row.quote) ?? ""}>
            <td class="left">{row.quote.threshold}+</td>
            <td>{row.model === null ? "—" : `${Math.round(row.model * 100)}%`}</td>
            <td>{cents(row.quote.yesBid)}</td>
            <td>{cents(row.quote.yesAsk)}</td>
            <td>{cents(row.quote.mid)}</td>
            <td>{signedPoints(row.edge)}</td>
            <td>{signedPoints(row.noEdge)}</td>
            <td>{Math.round(row.quote.volume).toLocaleString()}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .kalshi-ladder header {
    display: flex;
    gap: 0.75rem;
    align-items: baseline;
  }
  .kalshi-ladder header button {
    margin-left: auto;
  }
  .kalshi-ladder table {
    width: 100%;
    border-collapse: collapse;
  }
  .kalshi-ladder th,
  .kalshi-ladder td {
    text-align: right;
    padding: 0.25rem 0.5rem;
  }
  .kalshi-ladder .left {
    text-align: left;
  }
  .kalshi-ladder tr.thin {
    opacity: 0.45;
  }
</style>
