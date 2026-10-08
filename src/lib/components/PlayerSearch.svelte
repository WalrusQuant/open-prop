<script lang="ts">
  import { tick } from "svelte";
  import type { PlayerOption } from "$lib/types";

  let {
    players,
    selectedId,
    onSelect,
  }: {
    players: PlayerOption[];
    selectedId: number | null;
    onSelect: (playerId: number) => void;
  } = $props();

  let query = $state("");
  let open = $state(false);
  let active = $state(0);
  let list = $state<HTMLUListElement | undefined>();

  let matches = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    const selected = players.find((player) => player.playerId === selectedId);
    const showingSelected = selected != null && needle === selected.name.toLowerCase();
    if (!needle || showingSelected) return players;
    return players.filter((player) => `${player.name} ${player.team}`.toLowerCase().includes(needle));
  });

  $effect(() => {
    const selected = players.find((player) => player.playerId === selectedId);
    if (selected && !open) query = selected.name;
  });

  function choose(player: PlayerOption) {
    query = player.name;
    open = false;
    onSelect(player.playerId);
  }

  async function move(next: number) {
    active = next;
    await tick();
    list?.querySelectorAll<HTMLElement>("[role='option']")[next]?.scrollIntoView({ block: "nearest" });
  }

  function onKey(event: KeyboardEvent) {
    if (!open && (event.key === "ArrowDown" || event.key === "Enter")) {
      open = true;
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      void move(Math.min(active + 1, Math.max(matches.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      void move(Math.max(active - 1, 0));
    } else if (event.key === "Enter" && matches[active]) {
      event.preventDefault();
      choose(matches[active]);
    } else if (event.key === "Escape") {
      open = false;
    }
  }
</script>

<div class="search">
  <label for="player-search">Player</label>
  <input
    id="player-search"
    role="combobox"
    aria-expanded={open}
    aria-controls="player-list"
    aria-autocomplete="list"
    placeholder="Find a player"
    bind:value={query}
    onfocus={() => (open = true)}
    oninput={() => {
      open = true;
      active = 0;
    }}
    onkeydown={onKey}
    onblur={() => {
      setTimeout(() => (open = false), 120);
    }}
  />
  {#if open}
    <ul
      id="player-list"
      role="listbox"
      bind:this={list}
      onmousedown={(event) => event.preventDefault()}
    >
      {#if matches.length === 0}
        <li class="empty">No cached player matches.</li>
      {:else}
        {#each matches as player, index (player.playerId)}
          <li>
            <button
              type="button"
              role="option"
              aria-selected={index === active}
              class:active={index === active}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => choose(player)}
            >
              <span>{player.name}</span>
              <span class="meta">{player.team} · {player.games}</span>
            </button>
          </li>
        {/each}
      {/if}
    </ul>
  {/if}
</div>

<style>
  .search {
    position: relative;
  }

  label {
    display: block;
    margin-bottom: 0.3rem;
    font-family: var(--font-mono);
    font-size: 0.72rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  input {
    width: 100%;
    height: 2.4rem;
    padding: 0 0.7rem;
    background: var(--panel);
    color: var(--ink);
    border: 1px solid var(--rule);
    border-radius: 8px;
    font: inherit;
  }

  ul {
    position: absolute;
    z-index: 4;
    left: 0;
    right: 0;
    max-height: 18rem;
    margin: 0;
    padding: 0.25rem 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    list-style: none;
    background: var(--panel);
    border: 1px solid var(--rule);
    border-radius: 0 0 8px 8px;
    border-top: 0;
  }

  button {
    display: flex;
    width: 100%;
    height: auto;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.45rem 0.65rem;
    background: transparent;
    color: var(--ink);
    border: 0;
    border-radius: 0;
    font: inherit;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
  }

  button.active,
  button:hover {
    background: var(--chip);
  }

  .meta,
  .empty {
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.78rem;
  }

  .empty {
    padding: 0.45rem 0.65rem;
  }
</style>
