<script lang="ts">
  import { page } from "$app/stores";
  import { errorText, inTauri, loadModelScores } from "$lib/api";
  import { statParts } from "$lib/catalog";
  import { formatWhen } from "$lib/format";
  import { currentStatus, session } from "$lib/session.svelte";
  import type { TrainStatReport } from "$lib/types";

  let rows = $state<TrainStatReport[]>([]);
  let loadError = $state<string | null>(null);
  let desktop = $state(false);
  let request = 0;

  let statId = $derived($page.params.stat ?? "");
  let known = $derived(session.stats.some((item) => item.id === statId));
  let label = $derived(session.stats.find((item) => item.id === statId)?.label ?? statId);
  let row = $derived(rows.find((item) => item.stat === statId) ?? null);
  let status = $derived(currentStatus());
  let fitted = $derived(row != null && row.error == null && row.trainRows > 0);
  let parts = $derived(statParts(statId));
  let stale = $derived(
    row?.fittedAt != null && status?.syncedAt != null && row.fittedAt < status.syncedAt,
  );

  $effect(() => {
    desktop = inTauri();
    const season = session.season;
    const seasonType = session.seasonType;
    if (!desktop || !session.ready) return;
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

  function number(value: number | null, digits = 2): string {
    return value == null ? "—" : value.toFixed(digits);
  }

  function whole(value: number | null): string {
    return value == null ? "—" : Math.round(value).toLocaleString();
  }

  function cover(value: number | null): string {
    return value == null ? "—" : `${Math.round(value * 100)}%`;
  }

  function times(value: number | null): string {
    return value == null ? "—" : `${value.toFixed(3)}×`;
  }
</script>

<p class="back"><a href="/">Back</a></p>

{#if session.ready && !known}
  <header class="page-head">
    <div>
      <h1>Unknown stat</h1>
      <p>{statId} is not a stat this desk trains.</p>
    </div>
  </header>
{:else}
  <header class="page-head">
    <div>
      <h1>{label}</h1>
      <p>
        {session.season}
        {session.seasonType}.
        {#if row?.fittedAt}
          Fit {formatWhen(row.fittedAt)}.
        {:else if fitted}
          Fit.
        {:else}
          Not fit yet.
        {/if}
      </p>
    </div>
  </header>

  {#if loadError}
    <p class="banner bad">{loadError}</p>
  {/if}
  {#if row?.error && !row.error.startsWith("No trained model")}
    <p class="banner bad">{row.error}</p>
  {/if}
  {#if stale}
    <p class="banner">
      The cache was synced after this fit. Refit on the home page so this model sees the new games.
    </p>
  {/if}
  {#if !desktop}
    <p class="banner">The test results are in the desktop app.</p>
  {/if}

  <section class="panel">
    <h2>What this model is</h2>
    {#if parts}
      <p>
        {label} is the sum of {parts}. It is not a separate rate. Each part is a negative binomial
        count per minute,
        shrunk toward players who play similar minutes, with its own opponent, home, and rest
        multipliers. Points, rebounds, and assists share one minutes distribution. Once the minutes
        are fixed, the rates are separate. The chance at a line is that sum. The saved fit did not
        see the last 20% of dates. Those dates are the test, next to each player's last-10 total.
        A game clears a line when the stat is at or above it.
      </p>
    {:else}
      <p>
        {label} is one rate per minute for the whole season, with a gamma prior, not one model per
        player. A short sample shrinks toward players who play similar minutes. Each opponent has
        its own multiplier. Home and rest are two small multipliers. The chance at a line is a
        negative binomial: wider when the expected total is higher, and never below zero.
        {#if statId === "points"}For points, it is stretched about 15% around its middle, with a 5%
          chance of a short night at about a third of normal scoring.{/if} Last 5,
        last 10, and last 20 mix in a second rate. The weight on it is how much those games look
        like a real change. The season window uses the season rate. The saved fit did not see the
        last 20% of dates. Those dates are the test, next to each player's last-10 total. A game
        clears a line when the stat is at or above it.
      </p>
    {/if}
  </section>

  <section class="panel">
    <h2>Settings</h2>
    <dl class="facts">
      <div><dt>Prior minutes</dt><dd>{whole(row?.priorMinutes ?? null)}</dd></div>
      <div><dt>Opponent minutes</dt><dd>{whole(row?.opponentMinutes ?? null)}</dd></div>
      <div><dt>Shift prior</dt><dd>{number(row?.shiftPrior ?? null, 2)}</dd></div>
      <div><dt>Carry minutes</dt><dd>{whole(row?.carryMinutes ?? null)}</dd></div>
      <div><dt>Opponent carry</dt><dd>{whole(row?.opponentCarryMinutes ?? null)}</dd></div>
      <div><dt>Carry decay</dt><dd>{whole(row?.carryDecayTau ?? null)}</dd></div>
      {#if row?.seededFrom}
        <div><dt>Carried</dt><dd>{row.seededFrom}</dd></div>
      {/if}
      {#if row?.homeMultiplier != null}
        <div><dt>Home</dt><dd>{times(row.homeMultiplier)}</dd></div>
      {/if}
      {#if row?.restPerDay != null}
        <div><dt>Rest per day</dt><dd>{times(row.restPerDay)}</dd></div>
      {/if}
    </dl>
    <p class="proof">
      {#if parts}
        Home and rest are left off this page because the parts do not share one multiplier. The
        priors above are the ones in <code>models/specs/{statId}.json</code>, and the same numbers
        are used for each part. An edit takes effect the next time you refit.
      {:else if row?.settingsStored}
        These priors were stored with this fit. Home and rest per day are the multipliers the fit
        landed on. Edit the priors in <code>models/specs/{statId}.json</code> and refit.
      {:else}
        The spec lives in <code>models/specs/{statId}.json</code>. Refit on the home page to use it.
      {/if}
      {#if row?.seededFrom}
        This fit started each player from his {row.seededFrom}, worth at most the carry minutes, and
        each opponent from its multiplier there.
      {:else if fitted}
        This fit did not carry a season. Every player started from his minutes role.
      {/if}
    </p>
  </section>

  <section class="panel">
    <h2>Test</h2>
    {#if fitted && row}
      <dl class="facts">
        <div><dt>Holdout error</dt><dd>{number(row.holdoutMae)}</dd></div>
        <div><dt>Last 10 error</dt><dd>{number(row.baselineMae)}</dd></div>
        <div><dt>80% coverage</dt><dd>{cover(row.holdoutCoverage)}</dd></div>
        <div><dt>Train rows</dt><dd>{row.trainRows.toLocaleString()}</dd></div>
        <div><dt>Holdout rows</dt><dd>{row.holdoutRows.toLocaleString()}</dd></div>
      </dl>
      <p class="proof">
        Holdout error is the mean absolute error on the last 20% of dates. Last 10 error is the same
        games, using the last-10 total the model could see.
        {#if row.holdoutMae != null && row.baselineMae != null && row.holdoutMae > row.baselineMae}
          The last-10 average was closer on the holdout.
        {/if}
        Coverage is how often the actual stat landed in the middle 80% of the predictive distribution.
      </p>
    {:else if row?.seededFrom && row.error == null}
      <p>
        No {session.season} games to test on yet. This fit stands on {row.seededFrom}. The test fills
        in once the season has games and you refit.
      </p>
    {:else}
      <p>No test yet. Refit the season on the home page.</p>
    {/if}
  </section>
{/if}
