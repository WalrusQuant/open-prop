<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import Distribution from "$lib/components/Distribution.svelte";
  import PlayerSearch from "$lib/components/PlayerSearch.svelte";
  import Prediction from "$lib/components/Prediction.svelte";
  import TrendChart from "$lib/components/TrendChart.svelte";
  import { errorText, loadPlayers, loadTrend } from "$lib/api";
  import { lastSeasonSentence, teamNote } from "$lib/carry";
  import { chipOrder, statShort, windowShort } from "$lib/catalog";
  import { dnpSentence, downloadCsv, formatLine, formatStat, round1, shortDate } from "$lib/format";
  import { cachedSeed, carrying, playersReady, session, syncCurrent } from "$lib/session.svelte";
  import { INPUT_DELAY_MS, later } from "$lib/timing";
  import type { PlayerOption, Prediction as Projection, TrendGame, TrendReport } from "$lib/types";

  const BOX: { key: keyof TrendGame; stat: string }[] = [
    { key: "points", stat: "points" },
    { key: "rebounds", stat: "rebounds" },
    { key: "assists", stat: "assists" },
    { key: "steals", stat: "steals" },
    { key: "blocks", stat: "blocks" },
    { key: "turnovers", stat: "turnovers" },
  ];

  let players = $state<PlayerOption[]>([]);
  let playerId = $state<number | null>(null);
  let stat = $state("points");
  let windowId = $state("last_10");
  let line = $state(20);
  let lineHeld = $state(false);
  let report = $state<TrendReport | null>(null);
  let projection = $state<Projection | null>(null);
  let loading = $state(false);
  let deskError = $state<string | null>(null);
  let appliedUrl = $state("");
  let appliedMedian = $state("");
  let requestId = 0;
  let trendKey = "";
  let opponent = $state("");
  let site = $state("home");
  let rest = $state(1);
  let minutesText = $state("");
  let minutesFor = $state("");

  // With carry on, last season's players are listed before this season has a game.
  let synced = $derived(playersReady());
  let carriedSeason = $derived.by(() => {
    const seed = cachedSeed();
    return seed ? `${seed.season} ${seed.seasonType}` : null;
  });
  let mark = $derived(Number.isFinite(line) ? line : 0);
  let home = $derived(site === "home");
  let teams = $derived(
    [...new Set(players.map((player) => player.team).filter((team) => team.length > 0))].sort(
      (left, right) => left.localeCompare(right),
    ),
  );
  let minutesValue = $derived.by((): number | null | "bad" => {
    const text = minutesText.trim();
    if (!text) return null;
    const value = Number(text);
    if (!Number.isFinite(value) || value < 0 || value > 60) return "bad";
    return value;
  });
  let spotError = $derived(
    minutesValue === "bad"
      ? "Minutes have to be a number from 0 to 60."
      : !Number.isFinite(rest) || rest < 0
        ? "Rest has to be a number that is zero or greater."
        : null,
  );

  $effect(() => {
    if (opponent && !teams.includes(opponent)) opponent = "";
  });

  $effect(() => {
    const playerKey = `${session.season}|${session.seasonType}|${playerId}`;
    if (minutesFor === playerKey) return;
    minutesFor = playerKey;
    minutesText = "";
  });

  $effect(() => {
    if (!session.ready) return;
    const nextSeason = session.season;
    const nextType = session.seasonType;
    const carry = carrying();
    if (!playersReady()) {
      players = [];
      return;
    }
    loadPlayers(nextSeason, nextType, carry)
      .then((rows) => {
        if (nextSeason !== session.season || nextType !== session.seasonType) return;
        if (!playersReady()) return;
        players = rows;
      })
      .catch((caught: unknown) => {
        deskError = errorText(caught);
      });
  });

  $effect(() => {
    if (!players.length) return;
    const urlKey = $page.url.search;
    if (appliedUrl === urlKey) return;
    appliedUrl = urlKey;
    const params = $page.url.searchParams;
    const requestedStat = params.get("stat");
    const requestedWindow = params.get("window");
    const requestedPlayer = Number(params.get("player"));
    if (requestedStat && session.stats.some((item) => item.id === requestedStat)) {
      stat = requestedStat;
      lineHeld = false;
    }
    if (requestedWindow && session.windows.some((item) => item.id === requestedWindow)) {
      windowId = requestedWindow;
    }
    if (players.some((player) => player.playerId === requestedPlayer)) {
      playerId = requestedPlayer;
      lineHeld = false;
    }
    const requestedLine = Number(params.get("line"));
    if (params.has("line") && Number.isFinite(requestedLine) && requestedLine >= 0) {
      line = requestedLine;
      lineHeld = true;
    }
  });

  $effect(() => {
    if (!players.length) {
      playerId = null;
      return;
    }
    if (playerId != null && !players.some((player) => player.playerId === playerId)) {
      playerId = null;
    }
  });

  $effect(() => {
    if (!session.ready || playerId == null || !synced) return;
    const medianKey = `${session.season}|${session.seasonType}|${playerId}|${stat}`;
    const held = lineHeld;
    const currentLine = Number.isFinite(line) ? line : 0;
    const query = {
      season: session.season,
      seasonType: session.seasonType,
      playerId,
      stat,
      window: windowId,
      line: currentLine,
    };
    // A new player, stat, or window asks right away. Typing the number waits for a pause.
    const key = `${medianKey}|${windowId}`;
    const delay = key === trendKey ? INPUT_DELAY_MS : 0;
    trendKey = key;
    return later(() => requestTrend(query, medianKey, held, currentLine), delay);
  });

  function requestTrend(
    query: Parameters<typeof loadTrend>[0],
    medianKey: string,
    held: boolean,
    currentLine: number,
  ) {
    const id = ++requestId;
    loading = true;
    deskError = null;
    loadTrend(query)
      .then((next) => {
        if (id !== requestId) return;
        // Before his first game the number starts from last season's median.
        const median = next.summary.median ?? next.lastSeason?.median ?? null;
        if (!held && appliedMedian !== medianKey && median != null) {
          appliedMedian = medianKey;
          const suggested = round1(median);
          if (suggested !== currentLine) {
            line = suggested;
            return;
          }
        }
        appliedMedian = medianKey;
        report = next;
      })
      .catch((caught: unknown) => {
        if (id !== requestId) return;
        report = null;
        deskError = errorText(caught);
      })
      .finally(() => {
        if (id === requestId) loading = false;
      });
  }

  function selectPlayer(id: number) {
    playerId = id;
    lineHeld = false;
    const url = new URL(window.location.href);
    url.searchParams.set("player", String(id));
    url.searchParams.set("stat", stat);
    url.searchParams.set("window", windowId);
    void goto(`${url.pathname}${url.search}`, { replaceState: true, keepFocus: true, noScroll: true });
  }

  function useMedian() {
    const median = report?.summary.median ?? report?.lastSeason?.median ?? null;
    if (median == null) return;
    lineHeld = true;
    line = round1(median);
  }

  let note = $derived(report ? teamNote(report.teamSource, carriedSeason) : null);

  function signed(value: number, against: number): string {
    const delta = round1(value - against);
    if (delta === 0) return "0";
    const body = Number.isInteger(delta) ? String(Math.abs(delta)) : Math.abs(delta).toFixed(1);
    return delta > 0 ? `+${body}` : `−${body}`;
  }

  let orderedStats = $derived(chipOrder(session.stats));

  // Rust scores all four windows against the line in the same report.
  let splits = $derived(
    (report?.splits ?? []).map((item) => ({
      id: item.window,
      short: windowShort(item.window),
      overs: item.overs,
      sample: item.sample,
      rate: item.hitRate,
      low: item.wilsonLow,
      high: item.wilsonHigh,
      dnp: item.dnp,
    })),
  );

  let activeSplit = $derived(splits.find((item) => item.id === windowId) ?? null);

  let cuts = $derived.by(() => {
    if (!report) return [];
    const games = report.games;
    const count = (picked: typeof games) => ({
      overs: picked.filter((game) => game.over).length,
      sample: picked.length,
    });
    return [
      { label: "Home", ...count(games.filter((game) => game.location === "home")) },
      { label: "Away", ...count(games.filter((game) => game.location === "away")) },
      { label: "Back-to-back", ...count(games.filter((game) => game.restDays === 0)) },
      { label: "Rested", ...count(games.filter((game) => (game.restDays ?? 1) > 0)) },
    ];
  });
</script>

{#if !synced}
  <section class="empty">
    <h2>No games cached for {session.season}.</h2>
    <p>Sync pulls every player game log for this season type onto this machine.</p>
    <button type="button" onclick={syncCurrent} disabled={session.syncing}>
      {session.syncing ? "Syncing…" : `Sync ${session.season}`}
    </button>
  </section>
{:else}
  <form class="controls" onsubmit={(event) => event.preventDefault()}>
    <PlayerSearch {players} selectedId={playerId} onSelect={selectPlayer} />
    {#if playerId != null}
      {#if report}
        <header class="identity">
          <div>
            <p class="team">
              {report.team}
              {#if note}<span class="team-note">{note}</span>{/if}
              {#if report.noGames}<span class="team-note">No games this season yet</span>{/if}
            </p>
            <h1>{report.playerName}</h1>
          </div>
        </header>
      {/if}
      <div class="chips" role="group" aria-label="Stat">
        {#each orderedStats as item (item.id)}
          <button
            type="button"
            class="chip"
            class:on={stat === item.id}
            onclick={() => {
              stat = item.id;
              lineHeld = false;
            }}
          >
            {statShort(item.id, item.label)}
          </button>
        {/each}
      </div>
    {/if}
  </form>

  {#if playerId != null}
    <section class="number-room" aria-label="The number being checked">
      <p class="kicker">The number</p>
      <label class="number-edit">
        <input
          type="number"
          min="0"
          step="0.5"
          aria-label="The number to check"
          bind:value={line}
          oninput={() => {
            lineHeld = true;
          }}
        />
        <span class="number-unit">{(report?.statLabel ?? "points").toLowerCase()}</span>
      </label>
      <p class="number-help">
        Both cards below check this many {(report?.statLabel ?? "points").toLowerCase()} or more.
      </p>
      <div class="number-actions">
        <button type="button" class="ghost" onclick={useMedian}>Use the median</button>
        <button
          type="button"
          class="ghost"
          disabled={!report || report.noGames}
          onclick={() => report && downloadCsv(report)}
        >
          CSV
        </button>
      </div>
    </section>
  {/if}

  {#if deskError}
    <p class="banner bad">{deskError}</p>
  {/if}

  {#if report}
    <div class="rooms">
    <section class="room model-room">
      <header>
        <p class="kicker">Model</p>
        <h2>What the model expects for this game</h2>
      </header>
      <div class="fields">
        <label>
          Opponent
          <select bind:value={opponent}>
            <option value="">Average opponent</option>
            {#each teams as team (team)}
              <option value={team}>{team}</option>
            {/each}
          </select>
        </label>
        <label>
          Home or away
          <select bind:value={site}>
            <option value="home">Home</option>
            <option value="away">Away</option>
          </select>
        </label>
        <label>
          Days of rest
          <input type="number" min="0" max="14" step="1" bind:value={rest} />
        </label>
        <label>
          Minutes
          <input
            type="number"
            min="0"
            max="60"
            step="0.1"
            placeholder={report.noGames ? "Last season" : "Last 10"}
            value={minutesText}
            oninput={(event) => {
              minutesText = event.currentTarget.value;
            }}
          />
        </label>
      </div>
      <Prediction
        {playerId}
        {stat}
        line={mark}
        season={session.season}
        seasonType={session.seasonType}
        {opponent}
        {home}
        restDays={rest}
        minutes={minutesValue === "bad" ? null : minutesValue}
        {windowId}
        {spotError}
        trendMean={report.summary.mean}
        windowLabel={report.windowLabel}
        onprojection={(value) => {
          projection = value;
        }}
      />
      <Distribution
        games={report.games}
        line={mark}
        trendMean={report.summary.mean}
        windowLabel={report.noGames ? "No games yet" : report.windowLabel}
        statLabel={report.statLabel}
        mean={projection?.mean ?? null}
        pmf={projection?.pmf ?? null}
      />
    </section>

    <section class="room trend-room">
      <header>
        <p class="kicker">Trend</p>
        <h2>How often the games were {formatLine(report.line)} {(report.statLabel).toLowerCase()} or more</h2>
      </header>
    {#if report.noGames}
      <p class="band">No games this season yet.</p>
      {#if report.lastSeason}
        <p class="cuts">{lastSeasonSentence(report.lastSeason, report.line)}</p>
      {/if}
    {:else}
    <div class="splits" role="group" aria-label="How often the games reached the number">
      {#each splits as item (item.id)}
        <button
          type="button"
          class="split"
          class:on={windowId === item.id}
          aria-pressed={windowId === item.id}
          onclick={() => {
            windowId = item.id;
          }}
        >
          <span class="k">{item.short}</span>
          <strong>{item.rate == null ? "—" : `${(item.rate * 100).toFixed(0)}%`}</strong>
          <em>{item.sample ? `${item.overs}/${item.sample}` : "—"}</em>
        </button>
      {/each}
    </div>

    {#if activeSplit && activeSplit.sample > 0}
      <p class="band">
        {activeSplit.overs} of {activeSplit.sample} were {formatLine(report.line)} or more.
        {#if activeSplit.low != null && activeSplit.high != null}
          A 95% interval on that rate is {(activeSplit.low * 100).toFixed(0)}–{(activeSplit.high * 100).toFixed(0)}%.
        {/if}
        {#if activeSplit.sample < 30}
          {activeSplit.sample} games is a small sample, so that interval is wide.
        {/if}
        {#if activeSplit.dnp > 0}
          {dnpSentence(activeSplit.dnp)}
        {/if}
      </p>
    {/if}

    {#if cuts.length > 0}
      <p class="cuts">
        {#each cuts as cut (cut.label)}
          <span>{cut.label} <b>{cut.sample ? `${cut.overs}/${cut.sample}` : "—"}</b></span>
        {/each}
      </p>
    {/if}

    <section class="panel">
      <h2>{report.windowLabel}, and whether each was {formatLine(report.line)} or more</h2>
      <TrendChart
        games={report.games}
        line={report.line}
        label={`${report.playerName}, ${report.summary.overs} of ${report.summary.sample} cleared ${formatLine(report.line)} ${report.statLabel}`}
      />
      <p class="legend">
        <span class="key"><i class="swatch over"></i> {formatLine(report.line)} or more</span>
        <span class="key"><i class="swatch under"></i> Under {formatLine(report.line)}</span>
        <span class="key"><i class="swatch avg"></i> Last 3</span>
        <span class="key"><i class="swatch line"></i> At least {formatLine(report.line)}</span>
      </p>
      <dl class="strip">
        <div><dt>Mean</dt><dd>{formatStat(report.summary.mean)}</dd></div>
        <div><dt>Median</dt><dd>{formatStat(report.summary.median)}</dd></div>
        <div><dt>SD</dt><dd>{formatStat(report.summary.sd)}</dd></div>
        <div><dt>Min</dt><dd>{formatStat(report.summary.min, 0)}</dd></div>
        <div><dt>Max</dt><dd>{formatStat(report.summary.max, 0)}</dd></div>
      </dl>
    </section>

    <ol class="results" aria-label="Games in this window, oldest first">
      {#each report.games as game (game.gameId)}
        <li class={game.over ? "over" : "under"}>
          <span class="when">{shortDate(game.gameDate)}</span>
          <strong>{formatStat(game.stat, Number.isInteger(game.stat) ? 0 : 1)}</strong>
          <span class="opp">{game.location === "away" ? "@" : ""}{game.opponent}</span>
        </li>
      {/each}
    </ol>

    <div class="table-wrap">
      <table>
        <caption>Newest first.</caption>
        <thead>
          <tr>
            <th class="left">Date</th>
            <th class="left">Opp</th>
            <th>Min</th>
            <th>{statShort(report.stat, report.statLabel)}</th>
            <th>vs {formatLine(report.line)}</th>
            {#each BOX as column (column.key)}
              {#if column.stat !== report.stat}
                <th>{statShort(column.stat)}</th>
              {/if}
            {/each}
            <th>+/−</th>
          </tr>
        </thead>
        <tbody>
          {#each [...report.games].reverse() as game (game.gameId)}
            <tr>
              <td class="left">{shortDate(game.gameDate)}</td>
              <td class="left">{game.location === "away" ? "@" : ""}{game.opponent}</td>
              <td>{game.minutes.toFixed(1)}</td>
              <td class="stat">{formatStat(game.stat, Number.isInteger(game.stat) ? 0 : 1)}</td>
              <td class="mark {game.over ? 'over' : 'under'}">{signed(game.stat, report.line)}</td>
              {#each BOX as column (column.key)}
                {#if column.stat !== report.stat}
                  <td>{game[column.key]}</td>
                {/if}
              {/each}
              <td>{game.plusMinus}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {/if}
    </section>
    </div>
  {:else if playerId == null}
    <p class="waiting">Pick a player.</p>
  {:else if loading || players.length === 0}
    <p class="waiting">{players.length === 0 ? "Loading the roster…" : "Counting the window…"}</p>
  {/if}
{/if}
