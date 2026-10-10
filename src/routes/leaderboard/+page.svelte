<script lang="ts">
  import { goto } from "$app/navigation";
  import { errorText, loadBoard } from "$lib/api";
  import { defaultLine, statShort } from "$lib/catalog";
  import { formatLine, formatStat, formatWhen } from "$lib/format";
  import { cents, kalshiLists, rungAt } from "$lib/kalshi";
  import { kalshi, loadKalshiRows, pullKalshi } from "$lib/kalshi.svelte";
  import { currentStatus, session, syncCurrent } from "$lib/session.svelte";
  import { INPUT_DELAY_MS, later } from "$lib/timing";
  import type { BoardRow, BoardSplit } from "$lib/types";

  const FLOORS = [
    { id: "70", label: "70% of L10", rate: 0.7 },
    { id: "60", label: "60% of L10", rate: 0.6 },
    { id: "80", label: "80% of L10", rate: 0.8 },
    { id: "all", label: "Everyone", rate: 0 },
  ];

  const MIN_GAMES = 10;

  let stat = $state("points");
  let line = $state(defaultLine("points"));
  let floorId = $state("70");
  let search = $state("");
  let rows = $state<BoardRow[]>([]);
  let loading = $state(false);
  let boardError = $state<string | null>(null);
  let requestId = 0;
  let boardKey = "";
  let synced = $derived((currentStatus()?.games ?? 0) > 0);
  let floor = $derived(FLOORS.find((item) => item.id === floorId) ?? FLOORS[0]);
  let shown = $derived.by(() => {
    const needle = search.trim().toLowerCase();
    return rows.filter((row) => {
      if (needle && !`${row.name} ${row.team}`.toLowerCase().includes(needle)) return false;
      if (floor.rate <= 0) return true;
      if (row.last10.games < 8) return false;
      return row.last10.overs / row.last10.games >= floor.rate;
    });
  });

  let listed = $derived(kalshiLists(stat));

  $effect(() => {
    if (!kalshi.loaded) void loadKalshiRows();
  });

  function kalshiCell(row: BoardRow): { text: string; note: string; thin: boolean } {
    if (!listed) return { text: "no market", note: "", thin: false };
    const rung = rungAt(kalshi.rows, row.playerId, stat, Number.isFinite(line) ? line : 0);
    if (!rung) return { text: "no market", note: "", thin: false };
    return { text: cents(rung.yesAsk), note: `mid ${cents(rung.mid)}`, thin: rung.thin };
  }

  $effect(() => {
    if (!session.ready || !synced) {
      rows = [];
      return;
    }
    const mark = Number.isFinite(line) ? Math.max(0, line) : 0;
    const query = {
      season: session.season,
      seasonType: session.seasonType,
      stat,
      minGames: MIN_GAMES,
      line: mark,
    };
    // A new season or stat asks right away. Typing the line waits for a pause.
    const key = `${query.season}|${query.seasonType}|${query.stat}`;
    const delay = key === boardKey ? INPUT_DELAY_MS : 0;
    boardKey = key;
    return later(() => {
      const id = ++requestId;
      loading = true;
      boardError = null;
      loadBoard(query)
        .then((next) => {
          if (id !== requestId) return;
          rows = next;
        })
        .catch((caught: unknown) => {
          if (id !== requestId) return;
          boardError = errorText(caught);
        })
        .finally(() => {
          if (id === requestId) loading = false;
        });
    }, delay);
  });

  function shortStat(id: string): string {
    return statShort(id);
  }

  function pickStat(next: string) {
    stat = next;
    line = defaultLine(next);
  }

  function openPlayer(playerId: number) {
    const params = new URLSearchParams({
      player: String(playerId),
      stat,
      window: "last_10",
      line: String(Number.isFinite(line) ? line : 0),
    });
    void goto(`/player?${params.toString()}`);
  }

  function rate(split: BoardSplit): string {
    if (split.games === 0) return "—";
    return `${Math.round((100 * split.overs) / split.games)}%`;
  }
</script>

<header class="page-head">
  <div>
    <h1>Board</h1>
    <p>Who cleared the line. Sorted by the last 10.</p>
  </div>
  <div class="kalshi-head">
    <button type="button" onclick={() => pullKalshi(session.season)} disabled={kalshi.pulling}>
      {kalshi.pulling ? "Reading Kalshi…" : "Refresh Kalshi"}
    </button>
    <span>
      {kalshi.pulledAt ? `Kalshi read ${formatWhen(kalshi.pulledAt)}` : "Kalshi not read yet"}{kalshi.last &&
      kalshi.last.unmatched.length > 0
        ? ` · ${kalshi.last.unmatched.length} unmatched: ${kalshi.last.unmatched
            .slice(0, 5)
            .map((item) => item.name)
            .join(", ")}${kalshi.last.unmatched.length > 5 ? "…" : ""}`
        : ""}
    </span>
  </div>
</header>

{#if !synced}
  <section class="empty">
    <h2>Nothing to rank.</h2>
    <p>Sync {session.season} {session.seasonType} before the board has games to count.</p>
    <button type="button" onclick={syncCurrent} disabled={session.syncing}>
      {session.syncing ? "Syncing…" : `Sync ${session.season}`}
    </button>
  </section>
{:else}
  <div class="board-tools">
    <label>
      Player
      <input type="search" placeholder="Filter the list" bind:value={search} />
    </label>
    <label>
      Stat
      <select value={stat} onchange={(event) => pickStat(event.currentTarget.value)}>
        {#each session.stats as item (item.id)}
          <option value={item.id}>{shortStat(item.id)}</option>
        {/each}
      </select>
    </label>
    <label>
      Line
      <input type="number" min="0" step="0.5" bind:value={line} />
    </label>
    <label>
      Show
      <select bind:value={floorId}>
        {#each FLOORS as item (item.id)}
          <option value={item.id}>{item.label}</option>
        {/each}
      </select>
    </label>
  </div>
  {#if boardError}
    <p class="banner bad">{boardError}</p>
  {/if}
  {#if kalshi.error}
    <p class="banner bad">{kalshi.error}</p>
  {/if}
  {#if shown.length === 0 && !loading}
    <p class="waiting">
      {#if floor.rate > 0}
        Nobody with {MIN_GAMES}+ games cleared {formatLine(line)} {shortStat(stat)} in {floor.label}.
        Change the line, or show everyone.
      {:else}
        No player matches {search.trim() || "that filter"}.
      {/if}
    </p>
  {:else}
    <div class="table-wrap">
      <table>
        <caption>
          {shown.length.toLocaleString()}
          {shown.length === 1 ? "player" : "players"}.
          {floor.rate > 0 ? `${floor.label}, at least 8 games in the window.` : `At least ${MIN_GAMES} games.`}
          0-minute games are not counted.
          {loading ? "Updating." : ""}
        </caption>
        <thead>
          <tr>
            <th class="left">Player</th>
            <th class="left">Team</th>
            <th>L5</th>
            <th>L10</th>
            <th>L20</th>
            <th>Season</th>
            <th>Mean</th>
            <th title="Kalshi yes ask for {formatLine(line)}+ (free public data). Mid is the reference.">Kalshi {formatLine(line)}+</th>
          </tr>
        </thead>
        <tbody>
          {#each shown as row (row.playerId)}
            {@const cell = kalshiCell(row)}
            <tr>
              <td class="left">
                <button type="button" class="link" onclick={() => openPlayer(row.playerId)}>{row.name}</button>
              </td>
              <td class="left">{row.team}</td>
              <td class="rate"><strong>{rate(row.last5)}</strong><span>{row.last5.overs}/{row.last5.games}</span></td>
              <td class="rate"><strong>{rate(row.last10)}</strong><span>{row.last10.overs}/{row.last10.games}</span></td>
              <td class="rate"><strong>{rate(row.last20)}</strong><span>{row.last20.overs}/{row.last20.games}</span></td>
              <td class="rate">
                <strong>{rate(row.season)}</strong>
                <span>{row.season.overs}/{row.season.games}{row.dnp > 0 ? ` · ${row.dnp} DNP` : ""}</span>
              </td>
              <td>{formatStat(row.mean)}</td>
              <td class="rate kalshi" class:thin={cell.thin} title={cell.thin ? "Thin market: empty side, wide spread, or little volume." : ""}>
                <strong>{cell.text}</strong>{#if cell.note}<span>{cell.note}</span>{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
{/if}

<style>
  .kalshi-head {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    font-size: 0.85rem;
  }
  td.kalshi.thin {
    opacity: 0.45;
  }
</style>
