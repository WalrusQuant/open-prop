<script lang="ts">
  import { errorText, inTauri, loadPrediction } from "$lib/api";
  import { atLeast } from "$lib/deskMath";
  import { formatLine, formatStat } from "$lib/format";
  import { INPUT_DELAY_MS, later } from "$lib/timing";
  import type { Prediction } from "$lib/types";

  let {
    playerId,
    stat,
    line,
    season,
    seasonType,
    opponent,
    home,
    restDays,
    minutes,
    windowId = "last_10",
    spotError = null,
    trendMean = null,
    windowLabel = "",
    onprojection = () => {},
  }: {
    playerId: number | null;
    stat: string;
    line: number;
    season: string;
    seasonType: string;
    opponent: string;
    home: boolean;
    restDays: number;
    minutes: number | null;
    windowId?: string;
    spotError?: string | null;
    trendMean?: number | null;
    windowLabel?: string;
    onprojection?: (value: Prediction | null) => void;
  } = $props();

  let prediction = $state<Prediction | null>(null);
  let modelError = $state<string | null>(null);
  let shownFor = $state("");
  let request = 0;
  let askedFor = "";

  let desktop = $derived(inTauri());

  $effect(() => {
    const key = `${season}|${seasonType}|${playerId}|${stat}`;
    if (shownFor === key) return;
    shownFor = key;
    prediction = null;
    modelError = null;
    onprojection(null);
  });

  $effect(() => {
    const currentPlayer = playerId;
    const currentStat = stat;
    const currentSeason = season;
    const currentType = seasonType;
    const currentOpponent = opponent;
    const currentHome = home;
    const currentRest = restDays;
    const currentMinutes = minutes;
    const currentWindow = windowId;
    const currentSpotError = spotError;
    if (!desktop || currentPlayer == null) return;
    if (currentSpotError) {
      prediction = null;
      modelError = currentSpotError;
      onprojection(null);
      return;
    }
    const query = {
      season: currentSeason,
      seasonType: currentType,
      playerId: currentPlayer,
      stat: currentStat,
      line: 0,
      opponent: currentOpponent,
      home: currentHome,
      restDays: currentRest,
      minutes: currentMinutes,
      window: currentWindow,
    };
    // A new player, stat, or window asks right away. Typing rest or minutes waits for a pause.
    const key = `${currentSeason}|${currentType}|${currentPlayer}|${currentStat}|${currentWindow}`;
    const delay = key === askedFor ? INPUT_DELAY_MS : 0;
    askedFor = key;
    return later(() => {
      const id = ++request;
      loadPrediction(query)
        .then((next) => {
          if (id !== request) return;
          prediction = next;
          modelError = null;
          onprojection(next);
        })
        .catch((caught: unknown) => {
          if (id !== request) return;
          prediction = null;
          modelError = errorText(caught);
          onprojection(null);
        });
    }, delay);
  });

  let chance = $derived(
    prediction == null || prediction.pmf.length === 0
      ? null
      : atLeast(prediction.pmf, Number.isFinite(line) ? line : 0),
  );

</script>

<section class="chance">
  {#if prediction && chance != null}
    <div class="chance-body">
      <div class="chance-pct">
        <p class="says">Chance of {formatLine(line)} or more</p>
        <strong>{Math.round(chance * 100)}%</strong>
      </div>
      <dl class="facts">
        <div>
          <dt>Model expects</dt>
          <dd>{prediction.mean.toFixed(1)}</dd>
        </div>
        <div>
          <dt>Minutes</dt>
          <dd>{prediction.minutes.toFixed(1)}</dd>
        </div>
        {#if trendMean != null}
          <div>
            <dt>{windowLabel}</dt>
            <dd>{formatStat(trendMean)}</dd>
          </div>
        {/if}
        <div>
          <dt>Usual range</dt>
          <dd>{Math.round(prediction.low)}–{Math.round(prediction.high)}</dd>
        </div>
        {#if prediction.shiftProbability != null}
          <div>
            <dt>New rate</dt>
            <dd>{Math.round(prediction.shiftProbability * 100)}%</dd>
          </div>
        {/if}
      </dl>
    </div>
  {:else if modelError}
    <p class="quiet">
      {modelError}
      {#if modelError.startsWith("No trained model")}
        <a href="/">Train the season on the home page.</a>
      {/if}
    </p>
  {:else if desktop}
    <p class="quiet">Loading the model.</p>
  {:else}
    <p class="quiet">The probability shows in the desktop app after the season is trained.</p>
  {/if}
</section>
