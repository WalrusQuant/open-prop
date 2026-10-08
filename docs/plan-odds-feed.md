# Plan: player-prop odds feed (The Odds API)

**Status:** plan only — no implementation in this PR.  
**App:** open-prop (Tauri 2 + SvelteKit, no backend/daemon). Main at `6daa2db` when this was written.  
**Provider:** [The Odds API](https://the-odds-api.com/) v4.

This plan fits odds into a desktop app that only pulls when it opens or the user asks. There is no always-on scraper. Line “history” is whatever snapshots the app happened to store.

---

## 1. Sources (cite these; do not invent numbers)

| Topic | URL |
|---|---|
| v4 API guide (sports, odds, events, event odds, scores, historical, headers, costs) | https://the-odds-api.com/liveapi/guides/v4/ |
| Betting market keys (NBA player props + alternates) | https://the-odds-api.com/sports-odds-data/betting-markets.html |
| NBA sport guide (sport key, event-odds examples) | https://the-odds-api.com/sports/nba-odds.html |
| Bookmakers by region (`us`, `us2`, `uk`, `eu`, `au`, …) | https://the-odds-api.com/sports-odds-data/bookmaker-apis.html |
| Update intervals | https://the-odds-api.com/sports-odds-data/update-intervals.html |
| Historical overview | https://the-odds-api.com/historical-odds-data/ |
| FAQs (billing, credit reset) | https://the-odds-api.com/manage/faqs.html |
| Homepage pricing tiers (extracted 2026-10-08) | https://the-odds-api.com/ |
| open-prop stat catalog | `src/lib/catalog.json` |

**Not documented (call out, do not guess):** exact overage behavior beyond rate-limit `429`; whether every US book always returns every NBA prop key; a market key for **field goals attempted** (FGA). Homepage FAQ answers for “How frequently are odds updated?” and “How do I upgrade…” defer to other pages (linked above).

---

## 2. API surface we will use

**Host:** `https://api.the-odds-api.com` ([docs](https://the-odds-api.com/liveapi/guides/v4/)).

| Call | Path | Cost (documented) | Role in open-prop |
|---|---|---|---|
| Sports | `GET /v4/sports` | **0** | Optional sanity check; NBA key is `basketball_nba` |
| Events | `GET /v4/sports/{sport}/events` | **0** | Today’s / upcoming NBA slate (ids, teams, `commence_time`) |
| Event odds | `GET /v4/sports/{sport}/events/{eventId}/odds` | **markets_returned × regions** | Player props per game |
| Event markets | `GET .../events/{eventId}/markets` | **1** | Optional discovery of which props a book opened |
| Featured odds | `GET /v4/sports/{sport}/odds` | markets × regions | **Not** for props (h2h/spreads/totals only); skip for MVP |
| Historical event odds | `GET /v4/historical/sports/{sport}/events/{eventId}/odds` | **10 × markets_returned × regions** | Paid plans only; too expensive for routine line charts — prefer local snapshots |
| Historical events | `GET /v4/historical/sports/{sport}/events` | **1** (0 if empty) | Only if we ever backfill |

**Response headers (every call):** `x-requests-remaining`, `x-requests-used`, `x-requests-last` ([docs](https://the-odds-api.com/liveapi/guides/v4/)).

**Event-odds outcome shape (player props):** for each bookmaker market, outcomes include `name` (`Over` / `Under`), `description` (player display name), `price`, `point` (line). Example in the [v4 guide](https://the-odds-api.com/liveapi/guides/v4/#get-event-odds) and [NBA page](https://the-odds-api.com/sports/nba-odds.html).

**Regions:** start with `us` (DraftKings, FanDuel, BetMGM, …). `us2` is a second region and **doubles** credit cost if both are requested. EU includes Pinnacle but player-prop coverage is documented as mainly US sports / US books ([markets page](https://the-odds-api.com/sports-odds-data/betting-markets.html)).

**Update cadence (provider-side):** additional markets (player props) refresh about **60s** pre-match and in-play ([update intervals](https://the-odds-api.com/sports-odds-data/update-intervals.html)). That is the floor on freshness; our TTL will be coarser to save credits.

**Quota reset:** “Usage credits are automatically reset on the first of every month.” ([FAQs](https://the-odds-api.com/manage/faqs.html)).

**Rate limit:** HTTP **429** — space retries over several seconds ([v4 guide](https://the-odds-api.com/liveapi/guides/v4/#rate-limiting-status-code-429)).

---

## 3. Market map: open-prop stats → Odds API keys

From [`src/lib/catalog.json`](../src/lib/catalog.json) (14 stats) and [NBA player props](https://the-odds-api.com/sports-odds-data/betting-markets.html#nba-ncaab-wnba-player-props-api):

| open-prop `id` | Odds API market key | Notes |
|---|---|---|
| `points` | `player_points` | Main line |
| `rebounds` | `player_rebounds` | |
| `assists` | `player_assists` | |
| `three_point_field_goals_made` | `player_threes` | |
| `points_assists_rebounds` | `player_points_rebounds_assists` | PRA |
| `points_rebounds` | `player_points_rebounds` | |
| `points_assists` | `player_points_assists` | |
| `assists_rebounds` | `player_rebounds_assists` | |
| `steals` | `player_steals` | |
| `blocks` | `player_blocks` | |
| `turnovers` | `player_turnovers` | |
| `field_goals_made` | `player_field_goals` | Labelled “Field Goals” in the API docs |
| `free_throws_made` | `player_frees_made` | |
| `field_goals_attempted` | **none documented** | `player_frees_attempts` exists for FTAs; **no FGA key** on the published NBA list |

**Alternates (phase 3):** `player_*_alternate` for the same families (e.g. `player_points_alternate`). Docs say these cover milestones (X+) and book “alternate” labels.

**MVP market set (recommended):** the five board chips first —  
`player_points,player_rebounds,player_assists,player_threes,player_points_rebounds_assists`  
(5 markets). Expand to the 13 documented markets once budget is proven.

---

## 4. Credit budget and plan tier

### Documented pricing ([homepage](https://the-odds-api.com/), extracted 2026-10-08)

| Plan | Price | Credits / month | Historical odds |
|---|---|---|---|
| Starter | Free | 500 | Not included (struck through on homepage) |
| 20K | **$30** | 20,000 | Yes |
| 100K | **$59** | 100,000 | Yes |
| 5M | $119 | 5,000,000 | Yes |
| 15M | $249 | 15,000,000 | Yes |

There is **no $39 tier** on the published homepage. Adam’s “$39–59” band maps to **$30 (20K)** or **$59 (100K)** only.

### Cost formula (live event odds)

From the [v4 event-odds section](https://the-odds-api.com/liveapi/guides/v4/#get-event-odds):

```text
cost = [number of unique markets returned] × [number of regions specified]
```

Empty responses do not count. Markets requested but absent from the response are not charged.

Historical event odds: **10×** that ([same guide](https://the-odds-api.com/liveapi/guides/v4/#get-historical-event-odds)).

Events list: **free**.

### Math for a typical 10-game night

Assume `regions=us` (1 region), all requested markets return.

| Pull shape | Markets | Credits / event | Credits / 10-game slate |
|---|---:|---:|---:|
| MVP (PTS/REB/AST/3PM/PRA) | 5 | 5 | **50** |
| All 13 mapped stats | 13 | 13 | **130** |
| MVP + same alts (phase 3) | 10 | 10 | **100** |
| MVP with `us,us2` | 5 | 10 | **100** |
| One historical MVP snapshot × 10 games | 5 | 50 | **500** |

**Refresh / TTL (app-side, not provider):** props update ~60s on the provider; we should not poll that often.

| Usage pattern | Pulls / night | Credits / night (MVP 50) | Credits / night (full 130) |
|---|---:|---:|---:|
| Open once | 1 | 50 | 130 |
| TTL 15 min × 4 hours | 16 | 800 | 2,080 |
| TTL 30 min × 4 hours | 8 | 400 | 1,040 |

**Monthly (illustrative):** ~25 NBA evenings the user actually opens the app.

| Pattern | MVP @ 50 | Full @ 130 |
|---|---:|---:|
| 1 pull / evening × 25 | 1,250 | 3,250 |
| 8 pulls / evening × 25 | 10,000 | 26,000 |
| 16 pulls / evening × 25 | 20,000 | 52,000 |

### Recommendation

- **Plan: $59 / 100K credits.** Leaves headroom for full 13-market pulls, a few refreshes per night, and occasional discovery/`markets` calls. The **$30 / 20K** plan only fits MVP + tight TTL (or ~1 pull/night on full markets); easy to blow on a heavy evening.
- **Default regions: `us` only.** Add `us2` or named `bookmakers=` only if Adam needs a specific book not in `us`.
- **Default TTL: 10–15 minutes** while the window is open; never auto-poll when the app is backgrounded (no daemon).
- **Do not use historical event odds for charts.** At 10× cost, one evening of “every 15 min for 4 hours” historical backfill for MVP × 10 games would be `16 × 500 = 8,000` credits. Instead, **append a local snapshot every live pull** (`pulled_at`). History only exists when the app ran.

Free 500 credits: enough to prototype (~10 MVP slate pulls), not for daily use.

---

## 5. Fit to open-prop (no backend)

### Pull lifecycle

1. User opens the app or taps **Refresh odds** (Board / Home).
2. If no API key → settings prompt; skip network.
3. If last successful pull’s `pulled_at` is within TTL → serve SQLite; show age.
4. Else:
   - `GET .../events` for `basketball_nba` (free), filter to tonight’s slate (`commenceTimeFrom` / `To` in local/America/Chicago day bounds, or keep “upcoming” and filter client-side).
   - For each event id (cap concurrent requests; backoff on 429):  
     `GET .../events/{id}/odds?regions=us&markets=...&oddsFormat=american`.
   - Write events + snapshot rows + quota log in one transaction.
5. Match `description` names → NBA `player_id` (see §7).
6. UI reads latest snapshot (and optional prior snapshots for sparkline).

Off days: events empty → no charge; UI says “No NBA games with odds right now.”

### SQLite (app data DB, same file family as game logs)

Suggested tables (names flexible):

```sql
-- Odds API event metadata (latest known)
CREATE TABLE odds_events (
  event_id TEXT PRIMARY KEY,
  sport_key TEXT NOT NULL,
  commence_time TEXT NOT NULL,
  home_team TEXT NOT NULL,
  away_team TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- One row per outcome per pull (line history = rows over pulled_at)
CREATE TABLE odds_snapshots (
  id INTEGER PRIMARY KEY,
  pulled_at TEXT NOT NULL,
  event_id TEXT NOT NULL,
  bookmaker_key TEXT NOT NULL,
  market_key TEXT NOT NULL,
  player_name TEXT NOT NULL,      -- outcome.description
  player_id INTEGER,             -- nullable until matched
  name TEXT NOT NULL,            -- Over / Under
  price REAL NOT NULL,           -- American or decimal as stored
  odds_format TEXT NOT NULL,     -- 'american' | 'decimal'
  point REAL,                    -- line
  bookmaker_last_update TEXT,
  UNIQUE (pulled_at, event_id, bookmaker_key, market_key, player_name, name, point)
);
CREATE INDEX idx_odds_snapshots_lookup
  ON odds_snapshots (player_id, market_key, pulled_at);
CREATE INDEX idx_odds_snapshots_event
  ON odds_snapshots (event_id, pulled_at);

CREATE TABLE odds_quota_log (
  id INTEGER PRIMARY KEY,
  at TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  credits_last INTEGER,
  credits_used INTEGER,
  credits_remaining INTEGER,
  http_status INTEGER,
  detail TEXT
);

-- Optional durable name map after human/auto confirm
CREATE TABLE odds_player_map (
  player_name TEXT PRIMARY KEY,
  player_id INTEGER NOT NULL,
  confirmed TEXT NOT NULL
);
```

**Line movement:** for a `(player_id, market_key, bookmaker, point)` (or main line only), chart `price` / fair prob over distinct `pulled_at`. Gaps mean the app was closed — label the chart “Only while Open Prop was open,” not exchange-grade history.

### API key storage

- User pastes key in Settings (never shipped in repo, never `.env` in git).
- Prefer **OS keychain** via a Tauri plugin (e.g. stronghold / keyring) if we already accept that dependency; otherwise **file under `app.path().app_data_dir()`** with restricted permissions (same directory as `open-prop.db` today in `lib.rs`), e.g. `odds_api_key` with mode `0600`.
- Commands read the key only in Rust; do not echo it back to the frontend after save (return masked `…xxxx`).
- Document in README: key is local to the machine; uninstall may wipe app data.

### Rust command layer

Mirror `nba.rs` / `commands.rs`:

- `src-tauri/src/odds.rs` — HTTP client (reuse wreq/Chrome TLS pattern if the Odds API needs it; their docs show plain HTTPS — verify in implementation; fallback to `reqwest`/`wreq` as needed), parse JSON, map errors.
- Tauri commands (async + `spawn_blocking` for DB, same as Batch A):
  - `odds_status` — key present?, last pull, remaining credits from last header.
  - `odds_set_key` / `odds_clear_key`
  - `odds_refresh` — events + per-event odds with TTL/budget guards.
  - `odds_for_player` — latest (and optional history) for Board/Player.
- Budget guard: refuse refresh if `remaining < estimated_cost` (events × markets × regions), with a clear message.
- Record every response’s quota headers into `odds_quota_log`.

### Price math (model vs book)

1. Take Over and Under American prices for the same `(player, market, point, book)`.
2. Convert to implied probabilities; **devig** (e.g. multiplicative: `p_fair = p_raw / (p_over + p_under)`). Document the chosen method in Method; keep it simple and deterministic.
3. Model side: existing `predict` → `clear_probability` is P(stat ≥ line) for the half-point / integer line the user set ([`fit.rs`](../src-tauri/src/engine/fit.rs)). For a book line `L`:
   - If open-prop’s `at_least` matches book convention for that `L`, use it directly for Over.
   - Under ≈ `1 - P(stat ≥ L)` or `P(stat ≤ L-ε)` depending on push rules; **half points** (most NBA props) have no push — Over is `stat > L`, which for half-lines equals `stat ≥ ceil(L)`. Confirm against the model’s `at_least` semantics in implementation tests.
4. **Edge (Over)** ≈ `model_p_over - fair_p_over` (or Kelly input later — out of scope for MVP).
5. Multi-book: show best Over / best Under prices, and a consensus fair (median or volume-weighted later). MVP: pick one preferred book + “best available.”
6. Alt lines: run the same math at each `point`; highlight the line that maximizes |edge| or matches the user’s Board line.

**Do not** add an odds feed that implies guaranteed edge; Method copy stays: model probability vs market, no automatic bet sizing.

### UI

**Board**

- Column or chip: book line, Over price, fair %, model %, edge %.
- Filter: “only props with odds,” sort by edge.
- Stale badge: “Odds as of 12m ago” / “Refresh.”
- Quota strip on Home: remaining credits (from last header).

**Player page**

- Next to the line control: book’s main line + Over/Under; button “Use book line.”
- Small sparkline of line/price if ≥2 snapshots exist.
- Unmatched name: show raw `description` + “Link to player…” using the cached roster.

**Settings**

- API key field, preferred book, market set (MVP vs full), TTL, region.

### Failure modes

| Case | Behavior |
|---|---|
| No key | Settings CTA; model UI unchanged |
| Quota exhausted (`remaining` 0 or 401/402 if returned) | Freeze pulls; show remaining; keep last snapshot |
| HTTP 429 | Exponential backoff; partial slate OK |
| Off day / empty events | Friendly empty state; 0 credits |
| Book missing a market | Charge only returned markets; show “—” |
| Late scratch | Odds may linger until book pulls them (~15 min after market `last_update` stops per docs); mark stale if `bookmaker_last_update` older than threshold |
| Name mismatch | Leave `player_id` null; exclude from Board edge sort until mapped |
| Stale TTL | Show data + age; don’t pretend live |

---

## 6. Phased rollout

### Phase 0 — Plan (this doc)

No code.

### Phase 1 — MVP

- Key storage + Settings.
- Events + event-odds for **5 MVP markets**, `regions=us`, American odds.
- SQLite events + snapshots + quota log.
- Name match v1 (exact + simple normalize).
- Board + Player: line, Over/Under, fair %, model %, edge %.
- TTL + budget guard.
- Fixture-based Rust tests (recorded JSON, no live key).

### Phase 2 — Local history

- Sparklines / “line moved X → Y since first pull today.”
- Optional export of snapshots.
- Still no historical API unless Adam explicitly wants a one-shot backfill tool (credit warning).

### Phase 3 — Alts + more markets

- Remaining mapped markets (13).
- `*_alternate` markets; alt-line picker on Player.
- Optional `bookmakers=` allowlist; optional second region with cost calculator in UI.
- Hardening name map (team-scoped, nicknames).

---

## 7. Player name matching

Odds API gives **display names** in `description`, not NBA person ids. open-prop keys players by `player_id` from stats.nba.com.

**v1 algorithm**

1. Normalize: uppercase, strip accents, collapse punctuation/suffixes (`Jr.`, `III`).
2. Exact match against season roster + seed-season names in SQLite.
3. If the event’s teams are known, restrict candidates to those two teams’ rosters (Ambiguous “Jordan” problem).
4. Unique fuzzy match (Levenshtein / token set) above a high threshold → auto-link + write `odds_player_map`.
5. Else leave unmatched for UI confirm.

**Not available:** Odds API `/participants` for NBA returns **teams**, not players ([docs](https://the-odds-api.com/liveapi/guides/v4/#get-participants)). Do not expect a player id feed from them.

---

## 8. Test plan

- Check in **redacted fixtures** under e.g. `src-tauri/tests/fixtures/odds/` (event list + one event-odds payload). No API keys in fixtures or CI.
- Unit tests: parse → rows; credit estimator; devig; edge vs a fixed model pmf; TTL short-circuit (mock clock); budget refuse.
- Name match tests with known roster fixtures.
- Optional `#[ignore]` live test behind env key (local only), same pattern as `live_*` NBA tests.
- UI: vitest for formatting (American odds, edge %) without network.

---

## 9. Open questions for Adam

1. **Plan:** Confirm **$59 / 100K** vs trying **$30 / 20K** with MVP-only markets and strict TTL.
2. **Books:** Prefer one book (e.g. DraftKings), best-of-`us`, or also `us2` / DFS (`us_dfs` — indicative only per docs)?
3. **MVP markets:** Five board chips only, or all 13 documented on day one?
4. **FGA:** Accept “no odds” for `field_goals_attempted`, or drop/hide that chip when odds mode is on?
5. **Key storage:** OS keychain plugin OK, or app-data file enough?
6. **Auto-refresh:** Only manual, or timer while Board is focused (TTL 10–15 min)?
7. **Historical API:** Ever needed for research backfills, or is local-snapshot history enough?
8. **Edge display:** Show raw edge % only, or also a muted “not advice” on Method/Board (compliance preference)?
9. **Timezone for “today’s slate”:** Always `America/Chicago` (user zone), or device local?
10. **In-play:** Include live games or pre-match only? (In-play burns credits faster if TTL is short.)

---

## 10. Suggested implementation order (when coding starts)

1. Fixtures + parse + schema migration.  
2. Key storage + `odds_refresh` budget/TTL.  
3. Name match + Board column.  
4. Player page + “use book line.”  
5. Snapshot sparkline.  
6. Alts / full market set.

---

## 11. Summary recommendation

| Choice | Recommendation |
|---|---|
| Provider | The Odds API v4, sport `basketball_nba` |
| Access pattern | On open / on demand; SQLite snapshots for history |
| Credits | Event odds = markets_returned × regions; events list free |
| Tier | **$59 / 100K / mo** (within Adam’s band; $30 only if MVP+strict TTL) |
| Regions | `us` only at first |
| MVP markets | PTS, REB, AST, 3PM, PRA (5) |
| History | Local `pulled_at` snapshots; avoid 10× historical endpoint for routine use |
| Gaps | No documented FGA market key; no player ids from API |

