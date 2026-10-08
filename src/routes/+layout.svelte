<script lang="ts">
  import "../app.css";
  import "@fontsource/instrument-sans/latin-400.css";
  import "@fontsource/instrument-sans/latin-500.css";
  import "@fontsource/instrument-sans/latin-600.css";
  import "@fontsource/ibm-plex-mono/latin-400.css";
  import "@fontsource/ibm-plex-mono/latin-500.css";
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { errorText } from "$lib/api";
  import { formatWhen } from "$lib/format";
  import {
    currentStatus,
    openDesk,
    seasonChoices,
    session,
    syncCurrent,
  } from "$lib/session.svelte";
  import { initTheme, theme, toggleTheme } from "$lib/theme.svelte";

  let { children } = $props();
  let booting = $state(true);
  let bootError = $state<string | null>(null);

  onMount(() => {
    initTheme();
    openDesk()
      .catch((caught: unknown) => {
        // Tauri rejects with the Rust error string, not an Error.
        bootError = errorText(caught) || "The cache could not be opened.";
      })
      .finally(() => {
        booting = false;
      });
  });

  let status = $derived(currentStatus());
  let path = $derived($page.url.pathname);
</script>

<div class="app">
  <header class="mast">
    <div class="brand">
      <a class="word" href="/">Open Prop</a>
      <nav>
        <a href="/" aria-current={path === "/" ? "page" : undefined}>Home</a>
        <a href="/player" aria-current={path === "/player" ? "page" : undefined}>Player</a>
        <a href="/leaderboard" aria-current={path === "/leaderboard" ? "page" : undefined}>Board</a>
        <a href="/method" aria-current={path === "/method" ? "page" : undefined}>Method</a>
      </nav>
    </div>
    <div class="tools">
      <label>
        Season
        <select
          value={session.season}
          onchange={(event) => {
            session.season = event.currentTarget.value;
            session.notice = null;
            session.warning = null;
            session.error = null;
          }}
        >
          {#each seasonChoices() as item (item)}
            <option value={item}>{item}</option>
          {/each}
        </select>
      </label>
      <label>
        Type
        <select
          value={session.seasonType}
          onchange={(event) => {
            session.seasonType = event.currentTarget.value;
            session.notice = null;
            session.warning = null;
            session.error = null;
          }}
        >
          <option>Regular Season</option>
          <option>Playoffs</option>
        </select>
      </label>
      <button type="button" onclick={syncCurrent} disabled={session.syncing || !session.ready}>
        {session.syncing ? "Syncing…" : "Sync"}
      </button>
      <button type="button" class="ghost" onclick={toggleTheme}>
        {theme.mode === "dark" ? "Light" : "Dark"}
      </button>
    </div>
    <p class="sync-note">
      {#if status?.syncedAt}
        {status.games.toLocaleString()} games cached {formatWhen(status.syncedAt)}
      {:else if session.ready}
        This season is not cached yet.
      {:else}
        Opening the local cache…
      {/if}
    </p>
  </header>

  {#if session.preview}
    <p class="banner">Preview data is invented. The installed app reads stats.nba.com.</p>
  {/if}
  {#if bootError}
    <p class="banner bad">{bootError}</p>
  {/if}
  {#if session.error}
    <p class="banner bad">{session.error}</p>
  {/if}
  {#if session.warning}
    <p class="banner warn">{session.warning}</p>
  {/if}
  {#if session.notice}
    <p class="banner">{session.notice}</p>
  {/if}

  <main>
    {#if booting}
      <p class="waiting">Opening the local cache…</p>
    {:else}
      {@render children()}
    {/if}
  </main>

  <footer>
    <span>stats.nba.com</span>
    <span>SQLite on this machine</span>
    <span>A game is an over at or above the line</span>
  </footer>
</div>
