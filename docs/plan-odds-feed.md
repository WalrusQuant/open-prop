# Plan: player-prop odds feed (Kalshi + The Odds API)

**Status:** plan only — no implementation in this PR.  
**App:** open-prop (Tauri 2 + SvelteKit, no backend/daemon). Main at `6daa2db` when drafted; decisions below from Adam on PR #7.  
**Providers:** behind an `OddsProvider` trait — **Kalshi** (free, built in, no key, no credits; the out-of-the-box default) and **[The Odds API](https://the-odds-api.com/) v4** (paid, user's own key; sportsbook + DFS + exchanges). Kalshi added with Adam's approval, Oct 9 2026; sections 3–8 below are The Odds API and unchanged.

Odds are pulled when the app opens, the user refreshes, or (optional) a user-chosen poll interval fires **while the app is open**. There is no daemon. Line history is whatever snapshots the app stored under `pulled_at`.

---

## Decisions locked (Adam, PR #7)

| # | Decision |
|---|---|
| Plan | Design for **$59 / 100K credits/mo** |
| Markets | Ship **all documented** mappings; Settings toggles per market; live credit estimate |
| Regions | **`us`, `us2`, `us_dfs`, `us_ex`** (verified keys — not `us_exc`); Settings toggles per region |
| FGA | Show **no odds** (no documented market key) |
| API key | Settings page → file in Tauri app data dir, mode **0600**, masked UI; **no** OS keychain |
| Refresh | Manual default; optional recurring poll with min interval, credit guard, app-open only |
| Games | **Pregame only** — drop events with `commence_time` ≤ now |
| Kalshi (Oct 9 2026) | Add Kalshi as a free built-in provider and the default; The Odds API stays as the paid, opt-in provider |

---

## 0. Provider architecture (`OddsProvider`)

```rust
trait OddsProvider {
    fn id(&self) -> &'static str;              // "kalshi" | "the_odds_api"
    fn needs_key(&self) -> bool;               // kalshi: false
    fn estimate_cost(&self, slate: &Slate) -> Cost; // kalshi: Cost::Free
    async fn pull(&self, slate: &Slate) -> AppResult<Vec<Quote>>; // pregame only
}
```

- `Quote` is the normalized row written to `odds_snapshots` (section 9): event, player name + team, our stat key, threshold or line, side, price fields, `book`, `book_kind`, `pulled_at`.
- **Kalshi** is always on and needs no setup. **The Odds API** is enabled only when the user saves a key (sections 3–8 unchanged).
- One refresh runs every enabled provider. Each provider's failure is shown separately; one failing never blocks the other.

---

## 0b. Kalshi provider (free, default)

Survey run Oct 9 2026 against the public API `https://api.elections.kalshi.com/trade-api/v2` (`/series`, `/events`, `/markets`). Build waits for regular-season props (week of Oct 12–19).

### Series and coverage

| Series | open-prop stat | Observed |
|---|---|---|
| `KXNBAPTS` | PTS | Listed through the 2026 playoffs and for preseason games Oct 5 and Oct 8 |
| `KXNBAREB` | REB | Listed through the 2026 playoffs |
| `KXNBAAST` | AST | Listed through the 2026 playoffs |
| `KXNBA3PT` | 3PM | Listed through the 2026 playoffs |
| `KXNBASTL` | STL | Listed through the 2026 playoffs |
| `KXNBABLK` | BLK | Listed through the 2026 playoffs |
| `KXNBAFTM` | FTM | Occasional (7 events ever, Finals only) |
| `KXNBAPRA`, `KXNBAPA`, `KXNBAPR`, `KXNBARA` | PRA, P+A, P+R, R+A | Series exist but have **never** listed an event; poll them anyway |
| — | TOV, FGM, FGA | No Kalshi series; show "no Kalshi market" |

Game lines (`KXNBAGAME`, `KXNBASPREAD`, `KXNBATOTAL`) exist but aren't used by the prop board. On Oct 9 no prop markets were open; `KXNBAGAME` had 84 open markets across 42 games.

### Market structure → model

- One event per game per stat (for example `KXNBAPTS-26OCT08BOSCLE`), holding a yes/no **ladder** per player: ticker `…-BOSJTATUM0-25`, title `Jayson Tatum: 25+ points`, `floor_strike` 24.5, `strike_type` `greater`.
- Yes means stat ≥ ceil(`floor_strike`). That maps **directly** to the model's NB P(stat ≥ X), with X = `floor_strike` + 0.5. No devig and no half-point logic.
- Accept only `strike_type = greater`; skip and log anything else.

### Price and edge

- Store `yes_bid`, `yes_ask` (from the `*_dollars` string fields, 0–1), `volume_fp`, `open_interest_fp`, `close_time`.
- **Edge = model P(≥ X) − yes ask** (the price you would pay). Show **mid** = (bid + ask) / 2 as the market's probability reference.
- "No" side: edge = (1 − model P) − no ask, where no ask = 1 − yes bid.
- **Thin-market flag:** low `volume_fp`, missing bid or ask, or a wide spread (ask − bid). Thresholds get tuned from Phase 1 fixtures. Flagged rows are shown greyed out and excluded from "best edge" sorting.
- **Fees (unverified):** Kalshi's taker fee is believed to scale with P × (1 − P) per contract. I couldn't confirm the current schedule, so v1 shows edge before fees with a note. The tooltip links to Kalshi's fee page.

### Player matching

- Titles carry the full player name ("Paul George: 25+ points"), and the ticker carries a team code plus a short-name ID (`BOSPGEORGE13`).
- Match on **name + team** using the section 12 pipeline (normalize → team-scoped exact → high-threshold fuzzy → `odds_player_map`). The team comes from the ticker prefix.

### Call pattern per slate

1. For each prop series, call `GET /events?series_ticker=<S>&status=open`, following `cursor` until it comes back empty.
2. For each pregame event, call `GET /markets?event_ticker=<E>`, also paginated with `cursor`.
3. That's about 11 + N calls per slate. It's free, but keep the existing manual-refresh default and poll minimum, and throttle client-side, with backoff on HTTP 429.
4. Drop events whose `close_time` or game start has passed (pregame only, same rule as the Odds API).

### Storage

- Same `odds_snapshots` table keyed by `pulled_at`, with `bookmaker_key = 'kalshi'`, `book_kind = 'exchange'`, `market_key` = our stat key, `point` = threshold X, `name` = `yes`/`no`.
- Price fields: store yes ask as `price` (probability, `odds_format = 'prob'`) plus `bid`, `ask`, `volume` and `open_interest` columns (see section 9).
- Kalshi pulls don't write `odds_quota_log` credit fields; they log HTTP status only.

### Auth

- All of the reads above are **unauthenticated**. The Kalshi provider uses no key.
- Never read, copy or store any user Kalshi API key for this feature, and never place orders. open-prop is read-only.

### Caveats

- Not every game gets props (on Oct 9, MEM@CHI had game lines but no props). Show "no Kalshi market" instead of hiding the game.
- Ladder steps are coarse (5-point steps for points). Compare at Kalshi's thresholds, not at sportsbook lines.
- Combo series have never listed, so coverage beyond 6 or 7 stats is unproven until the regular season.

---

## 1. Sources (cite these; do not invent numbers)

| Topic | URL |
|---|---|
| v4 API guide | https://the-odds-api.com/liveapi/guides/v4/ |
| Betting market keys (NBA props + alternates) | https://the-odds-api.com/sports-odds-data/betting-markets.html |
| NBA sport guide | https://the-odds-api.com/sports/nba-odds.html |
| Bookmakers by region | https://the-odds-api.com/sports-odds-data/bookmaker-apis.html |
| Update intervals | https://the-odds-api.com/sports-odds-data/update-intervals.html |
| Historical overview | https://the-odds-api.com/historical-odds-data/ |
| FAQs (billing, credit reset on 1st of month) | https://the-odds-api.com/manage/faqs.html |
| Homepage pricing | https://the-odds-api.com/ |
| open-prop catalog | `src/lib/catalog.json` |

**Still not documented:** exact body/status when monthly quota is exhausted (beyond `429` rate limit); whether every book in a region always returns every NBA prop; a market key for **FGA**.

---

## 2. API surface

**Host:** `https://api.the-odds-api.com`.

| Call | Path | Cost (documented) | Role |
|---|---|---|---|
| Events | `GET /v4/sports/basketball_nba/events` | **0** | Slate ids / teams / `commence_time` |
| Event odds | `GET /v4/sports/basketball_nba/events/{eventId}/odds` | **markets_returned × regions** | Player props |
| Event markets | `GET .../events/{eventId}/markets` | **1** | Optional discovery |
| Historical event odds | `GET /v4/historical/.../events/{eventId}/odds` | **10 × markets_returned × regions** | Avoid for routine history |

**Headers:** `x-requests-remaining`, `x-requests-used`, `x-requests-last`.

**Outcome shape:** `name` (Over/Under), `description` (player), `price`, `point`.

**Pregame filter:** after fetching events (and again before each odds call), **exclude** any event with `commence_time <= now` (UTC compare). Do not request odds for in-play games. If a game tips during a poll cycle, skip it on the next tick.

**Provider update floor:** player props ~**60s** ([update intervals](https://the-odds-api.com/sports-odds-data/update-intervals.html)). Poll minimum in Settings should be **≥ 60s** (recommend default **5 minutes**, minimum **2 minutes** so we do not burn quota faster than the feed moves).

---

## 3. Verified region keys

From [bookmaker APIs](https://the-odds-api.com/sports-odds-data/bookmaker-apis.html). Adam’s `us_exc` is **`us_ex`** in the docs.

| Region key | Kind | Example bookmaker keys (not exhaustive) |
|---|---|---|
| **`us`** | US sportsbooks | `draftkings`, `fanduel`, `betmgm`, `betrivers`, `bovada`, `williamhill_us` (Caesars), `fanatics`, … |
| **`us2`** | Additional US sportsbooks | `espnbet` (theScore Bet), `fliff`, `hardrockbet`, `ballybet`, `rebet`, … |
| **`us_dfs`** | US DFS / pick’em | `prizepicks`, `underdog`, `pick6`, `betr_us_dfs` |
| **`us_ex`** | US exchanges / prediction markets | `kalshi`, `novig`, `polymarket`, `prophetx`, `betopenly` |

Each selected region key counts as **1** toward the credit formula when passed in `regions=`. Selecting all four → **×4** vs `us` alone.

**DFS note (docs):** “Odds on DFS sites can vary based on user selections, therefore odds are indicative only.” PrizePicks demons/goblins are mapped into `*_alternate` markets with assigned prices (goblins default odds, demons +100) per the same page.

**Default Settings:** all four regions **on**; user can turn any off. Live credit estimate uses the count of enabled regions.

---

## 4. Market map (ship all documented)

| open-prop `id` | Odds API key | In Settings toggle | Odds in UI |
|---|---|---|---|
| `points` | `player_points` | yes | yes |
| `rebounds` | `player_rebounds` | yes | yes |
| `assists` | `player_assists` | yes | yes |
| `three_point_field_goals_made` | `player_threes` | yes | yes |
| `points_assists_rebounds` | `player_points_rebounds_assists` | yes | yes |
| `points_rebounds` | `player_points_rebounds` | yes | yes |
| `points_assists` | `player_points_assists` | yes | yes |
| `assists_rebounds` | `player_rebounds_assists` | yes | yes |
| `steals` | `player_steals` | yes | yes |
| `blocks` | `player_blocks` | yes | yes |
| `turnovers` | `player_turnovers` | yes | yes |
| `field_goals_made` | `player_field_goals` | yes | yes |
| `free_throws_made` | `player_frees_made` | yes | yes |
| `field_goals_attempted` | — | n/a | **No odds** (always) |

**Default:** all 13 toggles **on**. User may enable e.g. only points + assists.

**Alternates (later phase):** `player_*_alternate` as separate toggles or a single “Include alternates” switch (each alternate key is another market in the credit product).

---

## 5. Credit math ($59 / 100K plan)

**Formula (live event odds):**  
`cost_per_event = (unique markets returned) × (regions specified)`  
`cost_per_pull ≈ cost_per_event × (pregame events requested)`  
([v4 event odds](https://the-odds-api.com/liveapi/guides/v4/#get-event-odds)). Empty responses free. Events list free.

**Homepage tier:** $59 → **100,000 credits/month**, historical included; reset on the **1st** ([FAQs](https://the-odds-api.com/manage/faqs.html), [homepage](https://the-odds-api.com/)).

### Full load: 13 markets × 4 regions × 10 games

If every market returns for every event:

| | Credits |
|---|---:|
| Per event | 13 × 4 = **52** |
| Per pull (10 games) | 52 × 10 = **520** |
| Pulls that fit in 100K (slate-only) | ⌊100000 / 520⌋ = **192** |
| 1 pull × 25 evenings / month | **13,000** |
| 8 pulls × 25 evenings | **104,000** (over budget) |
| 5 pulls × 25 evenings | **65,000** |
| Max pulls / night if using the whole month on one night | 192 |

So **full markets + all 4 regions** is fine for a few refreshes per night across a month, but **aggressive polling (e.g. every 5–15 min all evening) will exhaust 100K**. Settings toggles and the credit guard are required, not optional polish.

### How toggles change cost (10-game slate, assume all requested markets return)

| Markets on | Regions on | Credits / pull | Pulls in 100K | 25 nights × 1 pull | 25 nights × 8 pulls |
|---:|---:|---:|---:|---:|---:|
| 13 | 4 (`us,us2,us_dfs,us_ex`) | **520** | 192 | 13,000 | 104,000 |
| 13 | 2 (e.g. `us,us2` only) | 260 | 384 | 6,500 | 52,000 |
| 13 | 1 (`us`) | 130 | 769 | 3,250 | 26,000 |
| 5 | 4 | 200 | 500 | 5,000 | 40,000 |
| 5 | 1 | 50 | 2,000 | 1,250 | 10,000 |
| 2 (PTS+AST) | 4 | 80 | 1,250 | 2,000 | 16,000 |
| 2 | 1 | 20 | 5,000 | 500 | 4,000 |

**UI:** Settings and the Refresh control show  
`Estimate: N markets × R regions × G games ≈ C credits (D remaining)`.  
`G` = current pregame event count from the last events fetch (or “—” until fetched). Recompute whenever toggles change.

**Budget guard (locked):** pause auto-poll (and warn on manual refresh) when `x-requests-remaining` &lt; max(estimated next pull, user threshold). Default threshold suggestion: **2× estimated full slate** or a user field (e.g. 2,000).

---

## 6. Devig and edge by book type

open-prop’s model gives `clear_probability` ≈ P(stat clears the line). Compare to a **fair** market probability derived from book prices.

### A. Traditional sportsbooks (`us`, `us2`)

Two-way Over/Under at the same `point`:

1. Convert American (or decimal) prices → raw implied probs.  
2. **Multiplicative devig:** `p_fair = p_raw / (p_over + p_under)`.  
3. **Edge (Over)** = `model_p_over − p_fair_over` (and symmetric for Under).

Half-points: no push; align model `at_least` with book convention in tests.

### B. DFS / pick’em (`us_dfs`)

Docs: prices are **indicative**; entries often use fixed payouts or multipliers rather than a classic vigged two-way market. PrizePicks alternate mapping uses assigned odds for demons/goblins.

**Plan:**

- Still store `price` / `point` / side when present.  
- **Do not** apply the same multiplicative two-way devig unless both Over and Under exist at the same point with book-style prices.  
- If only one side (or multiplier-style): treat displayed implied prob as **indicative**, label UI **“DFS (indicative)”**, and either:
  - show model % vs that indicative implied **without** calling it “devigged fair,” or  
  - skip edge and show model % + DFS line only.  
- Prefer comparing model to **sportsbook fair** when both exist for the same player/line; use DFS as a line source / secondary quote.

Exact PrizePicks/Underdog outcome shapes should be confirmed against a recorded fixture in Phase 1 — docs do not publish a full DFS JSON schema beyond the general outcome fields and the demon/goblin note.

### C. Exchanges (`us_ex`)

Back (and sometimes lay) prices; optional `includeBetLimits`.  

**Plan:** use **back** Over/Under when both exist → same multiplicative devig as sportsbooks. If only one side, show indicative. Label **“Exchange”**. Kalshi/Polymarket sports structures may differ; fixture-drive parsing and disable edge when the pair is incomplete.

### UI grouping

Board/Player group or badge quotes: **Sportsbook** / **DFS** / **Exchange**. Default “best Over” / “best Under” sorts **sportsbooks first**; DFS/exchange opt-in for “best” or shown in an expanded row.

---

## 7. Settings (product)

### API key

- Entered on Settings; saved only from Rust.  
- Storage: file under `app.path().app_data_dir()` (same family as `open-prop.db`), e.g. `odds_api_key`, written with **`0600`** on Unix.  
- **Never** log the key; **never** return the full key to the frontend after save — only masked (`••••…abcd`) plus `present: true`.  
- Actions: **Test** (cheap call: `/events` or sports — prefer **0-credit** `/events`), **Replace**, **Clear**.  
- **Tradeoff (honest):** the file is **plaintext on disk**. Acceptable for a **single-user desktop** app on the user’s machine (same trust as the SQLite cache). Optional later: light **obfuscation** (xor/encoding) labeled clearly as **obfuscation, not security**. Never git, never `.env`.

### Toggles

- **Markets:** one switch per documented mapping (13).  
- **Regions:** `us`, `us2`, `us_dfs`, `us_ex`.  
- Live credit estimate from selection × pregame game count.

### Refresh

- Default: **manual** only.  
- Optional: recurring poll, interval ≥ minimum (recommend min **2 min**, default **5 min**), **only while app is open / window focused** (define: main window exists and not suspended).  
- Credit guard pauses polling when remaining &lt; threshold; banner explains why.

### Other

- Preferred sportsbook key (optional) for primary quote.  
- Show DFS / exchange columns (off by default for edge sort).

---

## 8. Pull lifecycle

1. Load Settings (key, toggles, poll).  
2. Manual refresh or poll tick → if no key or guard tripped → stop with message.  
3. `GET .../events` (free) → filter **pregame only**.  
4. For each remaining event (concurrency limit, 429 backoff):  
   `GET .../odds?regions={enabled}&markets={enabled}&oddsFormat=american`.  
5. Transaction: upsert events, insert snapshot rows (`pulled_at`), append quota log.  
6. Match names → `player_id`.  
7. UI reads latest snapshot; history = prior `pulled_at` values.

Off day / all tipped: empty pregame list → no odds calls → 0 credits.

---

## 9. SQLite

```sql
CREATE TABLE odds_events (
  event_id TEXT PRIMARY KEY,
  sport_key TEXT NOT NULL,
  commence_time TEXT NOT NULL,
  home_team TEXT NOT NULL,
  away_team TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE odds_snapshots (
  id INTEGER PRIMARY KEY,
  pulled_at TEXT NOT NULL,
  event_id TEXT NOT NULL,
  bookmaker_key TEXT NOT NULL,
  region_key TEXT,                 -- us | us2 | us_dfs | us_ex when known
  book_kind TEXT,                  -- sportsbook | dfs | exchange
  market_key TEXT NOT NULL,
  player_name TEXT NOT NULL,
  player_id INTEGER,
  name TEXT NOT NULL,
  price REAL NOT NULL,
  odds_format TEXT NOT NULL,
  point REAL,
  bookmaker_last_update TEXT,
  bid REAL,                        -- kalshi yes bid (0–1)
  ask REAL,                        -- kalshi yes ask (0–1)
  volume REAL,                     -- kalshi volume_fp
  open_interest REAL,              -- kalshi open_interest_fp
  UNIQUE (pulled_at, event_id, bookmaker_key, market_key, player_name, name, point)
);

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

CREATE TABLE odds_player_map (
  player_name TEXT PRIMARY KEY,
  player_id INTEGER NOT NULL,
  confirmed TEXT NOT NULL
);
```

App settings (toggles, poll interval, threshold) can live in a small `odds_settings` table or a JSON file next to the key — prefer SQLite for one backup story.

---

## 10. Rust / UI sketch

- `src-tauri/src/odds.rs` + commands: `odds_status`, `odds_set_key`, `odds_clear_key`, `odds_test_key`, `odds_refresh`, `odds_for_board`, `odds_for_player`, `odds_estimate`.  
- Async + `spawn_blocking` for DB (same as existing reads).  
- Board: **Kalshi is its own column/source** (yes ask, mid, edge vs ask, thin flag; nearest ladder threshold to the model line), shown with no setup. Odds API books appear next to it when a key is set: line, prices by kind, fair % (sportsbooks), model %, edge %, stale age, FGA never shows odds.  
- Player: Kalshi ladder table (each X+ threshold: model P(≥X), yes bid/ask, mid, edge) plus book line, “Use book line,” sparkline from snapshots.  
- Home/Settings: masked key, toggles, estimate, remaining credits, poll controls.

---

## 11. Phased rollout (updated)

### Phase 1 — `OddsProvider` trait + Kalshi (free) — build week of Oct 12–19, once regular-season markets confirm

- Trait + normalized `Quote`; Kalshi provider (no key, no credits) as default.  
- Snapshots with `book = kalshi`; Kalshi Board column + Player ladder; thin flags; name + team match.  
- Recorded Kalshi fixtures; no network in tests.

### Phase 2 — The Odds API core feed

- Key file (0600) + Settings (markets, regions, estimate, poll, guard).  
- Pregame events + event-odds for all **enabled** documented markets × regions.  
- Snapshots + quota log.  
- Name match v1.  
- Board + Player sportsbook quotes + multiplicative devig edge.  
- FGA: no odds affordance.  
- Fixture tests; no live key in CI.

### Phase 3 — History + DFS/exchange presentation

- Sparklines / line moved since first pull.  
- DFS indicative labeling; exchange back-price handling from fixtures.  
- Credit dashboard (pulls today, projected month).

### Phase 4 — Alternates

- `*_alternate` toggles; alt-line picker; edge across lines.

---

## 12. Player name matching

Unchanged: normalize → roster/team-scoped exact → high-threshold fuzzy → `odds_player_map`. Odds API `/participants` for NBA is **teams**, not players.

---

## 13. Test plan

- Redacted fixtures for `us` sportsbook props, and separate fixtures for `us_dfs` / `us_ex` once captured.  
- Tests: parse, credit estimator (markets × regions × games), pregame filter, multiplicative devig, DFS “no false devig,” budget guard, TTL/poll min.  
- **Kalshi fixtures** (recorded, unauthenticated): an `/events` page with a `cursor`, a `/markets` ladder for the settled `KXNBAPTS-26OCT08BOSCLE` (for example `…-BOSPGEORGE13-25`, `floor_strike` 24.5, `strike_type` greater), an empty-series response (combo series), and a game with no props.  
- Kalshi tests: ticker/title parse → player + team + X; X = floor_strike + 0.5 maps to model P(≥X); edge vs ask, mid, no-side math; thin-market flags; pregame filter; pagination; non-`greater` strike skipped.  
- No API keys in repo (Kalshi needs none).

---

## 14. Remaining open questions

1. **Poll defaults:** confirm minimum **2 min** / default **5 min**, and whether polling requires window focus or merely “process running with main window open.”  
2. **Credit-guard default:** prefer fixed floor (e.g. 2,000 remaining) or **2× next-pull estimate**?  
3. **DFS edge:** OK to show **indicative only** (no edge %) until fixtures prove a clean two-way market, or always show a non-devigged “model vs DFS implied” delta with a warning?  
4. **Best price:** sportsbooks-only for “best Over/Under,” or include exchanges when both sides exist?  
5. **Timezone for “today” helpers** (if we add a day filter beyond “all pregame upcoming”): America/Chicago vs device local? (Pregame filter itself is absolute `commence_time`.)  
6. **Obfuscation:** ship plaintext key file only for v1, or add labeled obfuscation in Phase 1?

---

## 15. Summary

| Choice | Locked recommendation |
|---|---|
| Default provider | **Kalshi**, free, no key, read-only; PTS/REB/AST/3PM/STL/BLK (+ occasional FTM) |
| Tier (Odds API) | **$59 / 100K** |
| Regions | **`us`, `us2`, `us_dfs`, `us_ex`** (toggleable; all on by default) |
| Markets | All **13** documented mappings (toggleable); **FGA = no odds** |
| Full-slate cost | **13 × 4 × 10 = 520 credits/pull** if everything returns |
| Key | App-data file, **0600**, masked UI; no keychain; plaintext-on-disk tradeoff documented |
| Refresh | Manual default; optional in-app poll + credit guard |
| Games | **Pregame only** |
| History | Local snapshots; avoid 10× historical API for routine use |
