<script lang="ts">
  import { goto } from "$app/navigation";
  import PlayerSearch from "$lib/components/PlayerSearch.svelte";
  import { errorText, inTauri, loadModelScores, loadPlayers, trainModels } from "$lib/api";
  import { formatWhen } from "$lib/format";
  import { seedLabel, seedSource } from "$lib/season";
  import { carrying, currentStatus, playersReady, session, syncCurrent } from "$lib/session.svelte";
  import type { PlayerOption, TrainStatReport } from "$lib/types";

  let players = $state<PlayerOption[]>([]);
  let rows = $state<TrainStatReport[]>([]);
  let loadError = $state<string | null>(null);
  let desktop = $state(false);
  let request = 0;

  let status = $derived(currentStatus());
  let cached = $derived((status?.games ?? 0) > 0);
  let seedNote = $state<string | null>(null);
  let source = $derived(seedSource(session.season, session.seasonType));
  let sourceCached = $derived(
    source != null &&
      session.seasons.some(
        (item) => item.season === source.season && item.seasonType === source.seasonType && item.games > 0,
      ),
  );
  let sourceName = $derived(source ? `${source.season} ${source.seasonType}` : "");

  $effect(() => {
    desktop = inTauri();
  });

  // With carry on, last season's players are listed before this season has a game.
  let ready = $derived(session.ready && playersReady());
  let canFit = $derived(cached || carrying());

  $effect(() => {
    const season = session.season;
    const seasonType = session.seasonType;
    const carry = carrying();
    if (!ready) {
      players = [];
      return;
    }
    loadPlayers(season, seasonType, carry)
      .then((next) => {
        if (season !== session.season || seasonType !== session.seasonType) return;
        players = next;
      })
      .catch((caught: unknown) => {
        loadError = errorText(caught);
      });
  });

  $effect(() => {
    const season = session.season;
    const seasonType = session.seasonType;
    // A fit that started before this page mounted reloads the scores when it ends.
    const fitting = session.training;
    if (!desktop || !session.ready || fitting) return;
    const id = ++request;
    loadModelScores({ season, seasonType })
      .then((next) => {
        if (id !== request) return;
        rows = next;
        loadError = null;
      })
      .catch((caught: unknown) => {
        if (id !== request) return;
        loadError = errorText(caught);
      });
  });

  let stale = $derived(
    rows.some((row) => row.fittedAt != null && status?.syncedAt != null && row.fittedAt < status.syncedAt),
  );

  function number(value: number | null): string {
    return value == null ? "—" : value.toFixed(2);
  }

  function span(first: string | null, last: string | null): string {
    if (!first || !last) return "—";
    return `${day(first)} – ${day(last)}`;
  }

  function day(iso: string): string {
    const [year, month, date] = iso.split("-").map(Number);
    if (!year || !month || !date) return iso;
    return new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    }).format(new Date(year, month - 1, date));
  }

  function openPlayer(id: number) {
    const params = new URLSearchParams({ player: String(id) });
    void goto(`/player?${params.toString()}`);
  }

  async function train() {
    if (!desktop || session.training) return;
    session.training = true;
    loadError = null;
    seedNote = null;
    const season = session.season;
    const seasonType = session.seasonType;
    try {
      const report = await trainModels({ season, seasonType, seed: session.seed });
      if (season === session.season && seasonType === session.seasonType) {
        rows = report.stats;
        seedNote = report.seedNote;
      }
    } catch (caught: unknown) {
      loadError = errorText(caught);
    } finally {
      session.training = false;
    }
  }
</script>

<header class="page-head">
  <div>
    <h1>{session.season}</h1>
    <p>{session.seasonType}. Cached games on this machine, and the models fit on them.</p>
  </div>
</header>

{#if loadError}
  <p class="banner bad">{loadError}</p>
{/if}

<section class="panel">
  <h2>Cache</h2>
  {#if cached && status}
    <dl class="facts">
      <div>
        <dt>Games</dt>
        <dd>{status.games.toLocaleString()}</dd>
      </div>
      <div>
        <dt>Players</dt>
        <dd>{status.players.toLocaleString()}</dd>
      </div>
      <div>
        <dt>Dates</dt>
        <dd>{span(status.firstGame, status.lastGame)}</dd>
      </div>
      <div>
        <dt>Synced</dt>
        <dd>{status.syncedAt ? formatWhen(status.syncedAt) : "—"}</dd>
      </div>
    </dl>
  {:else}
    <p>Nothing cached for {session.season} {session.seasonType}.</p>
  {/if}
  <button type="button" onclick={syncCurrent} disabled={session.syncing || !session.ready}>
    {session.syncing ? "Syncing…" : cached ? "Sync again" : `Sync ${session.season}`}
  </button>
</section>

<section>
  <div class="page-head">
    <div>
      <h2>Models</h2>
      <p>One fit per stat for {session.season} {session.seasonType}. Open a stat for the test and what the model uses.</p>
    </div>
    <button type="button" onclick={train} disabled={!desktop || session.training || !session.ready || !canFit}>
      {session.training ? "Training…" : "Refit season"}
    </button>
  </div>

  {#if source}
    <label class="carry">
      {#if sourceCached}
        <input type="checkbox" bind:checked={session.seed} disabled={session.training} />
      {:else}
        <input type="checkbox" checked={false} disabled />
      {/if}
      Carry last season
      <span>
        {#if !sourceCached}
          Sync {sourceName} to start each player from it.
        {:else if session.seed && !cached}
          No {session.season} games yet. The fit stands on {sourceName}, so every player who played
          in it can be priced before his first game.
        {:else if session.seed}
          Each player starts from his {sourceName}.
        {:else}
          Every player starts from his minutes role.
        {/if}
      </span>
    </label>
  {/if}
  {#if session.training}
    <p class="sync-note">Refitting all 14 stats.</p>
  {/if}
  {#if seedNote}
    <p class="banner">{seedNote}</p>
  {/if}
  {#if stale}
    <p class="banner">The cache is newer than at least one fit. Refit so the models see the new games.</p>
  {/if}
  {#if !desktop}
    <p class="banner">Fits and the test results are in the desktop app.</p>
  {/if}

  {#if rows.length > 0}
    <ul class="cards">
      {#each rows as row (row.stat)}
        <li>
          <a href="/models/{row.stat}">
            <strong>{row.label}</strong>
            {#if row.error}
              <span class="when">{row.fittedAt ? `Fit ${formatWhen(row.fittedAt)}` : "Not fit"}</span>
              <span class="proof">
                {row.error.startsWith("No trained model") ? "Open for what this model is." : row.error}
              </span>
            {:else}
              <span class="when">
                {row.fittedAt ? `Fit ${formatWhen(row.fittedAt)}` : "Fit"}
                {#if row.fittedAt && status?.syncedAt && row.fittedAt < status.syncedAt}
                  · cache is newer
                {/if}
              </span>
              <span class="proof">
                Holdout {number(row.holdoutMae)} · last 10 {number(row.baselineMae)}
              </span>
              {#if row.seededFrom}
                <span class="when">Carried {seedLabel(row.seededFrom)}</span>
              {/if}
            {/if}
          </a>
        </li>
      {/each}
    </ul>
  {:else if desktop && !loadError}
    <p class="waiting">Reading saved models…</p>
  {:else}
    <ul class="cards">
      {#each session.stats as item (item.id)}
        <li>
          <a href="/models/{item.id}">
            <strong>{item.label}</strong>
            <span class="when">Not fit</span>
            <span class="proof">Open for what this model is.</span>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<section class="panel">
  <h2>Player</h2>
  {#if ready}
    <p>
      Open a player for the trend, a line, and the projection.
      {#if !cached}
        Nobody has a {session.season} game yet, so the list is {sourceName}, plus this season's
        rosters once a sync has them.
      {/if}
    </p>
    <PlayerSearch {players} selectedId={null} onSelect={openPlayer} />
  {:else}
    <p>Sync this season, then pick a player.</p>
  {/if}
</section>
