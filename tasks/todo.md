# Open Prop

## Build

- [x] Confirm `playergamelogs` and the Chrome TLS client
- [x] Tauri v2 + SvelteKit scaffold, icons, SQLite cache
- [x] Season calendar, parser, Wilson interval, commands
- [x] Prop board, hit-rate board, and method page
- [x] Rust tests, including one live 2025-26 sync
- [x] `pnpm check`, production build, and a pass through the preview in Chrome

## The prop screen

- Player name is the title. Stat chips are PTS, REB, AST, 3PM, PRA, and the rest.
- One line. L5, L10, L20, and the season are scored against that same line.
- Changing the window does not move the line. A new player or stat fills the median until the line is edited. Median puts the current window's median back.
- The chart is the games in the selected window against the line. Green cleared (`stat >= line`). Red missed.
- The Wilson band stays under the splits. Seven of ten is about 40% to 89%.
- Home, away, back-to-back, and rested are the same window counted again. There is no redraw and no minutes draw.
- All-Star ids (`003`) are dropped. Dunks & Threes and EPM files stay out of the source tree.

## The model

- [x] One rate per minute per counting stat. Role shrinkage, opponent factors, and small home and rest multipliers.
- [x] Predictive distribution is negative binomial. The player page sums that mass when the line changes.
- [x] Last 5, last 10, and last 20 mix in a second rate only when the shift test says so. Season does not.
- [x] Combos are sums of the parts. Points, rebounds, and assists share minutes uncertainty.
- [x] Hold out the last 20% of dates. Save the model that was scored, with its error and the last-10 error.
- [x] Specs in `models/specs`. Fitted models go to the app data folder. An old tree file asks for a refit.

## Pages

- [x] `/` is the cache and the models: games, players, date span, last sync, fit time, holdout versus last 10. Sync and refit live here.
- [x] `/models/[stat]` is that stat's overview and holdout test. A fit stores its time and settings. Older files use the file time and the spec on disk.
- [x] `/player` is the trend, the line, the probability, and the projection distribution against that window's mean.
- [x] Board and Method stay in the nav.

## Review

- The booster is gone. Each counting stat is a rate per minute, shrunk toward a minutes role, with an opponent factor and small home and rest multipliers. The player page sums the returned distribution when the line changes, so that edit does not refetch. Last 5, last 10, and last 20 report a "New rate" probability. The season window and the combos do not.
- `cargo test --lib`: 34 passed, 1 ignored (the live NBA sync). `pnpm check`: 0 errors. The desktop app was not reopened.
- Saved files from the tree model will not load. Refit the season on the home page. The expected totals may only move a little versus the last 10. The percent, and whether the recent games are a real change, are the part that changed.
- The roster effect reads the cached game count, so a sync that fills an empty season loads players. The preview never syncs a real cache, so this path was not re-opened in the Tauri webview.
- Split rates use the season log and `stat >= line`. The chart, moving average, and table still come from the Rust report for the selected window.
- A push (`stat == line`) counts as cleared and shows `0` in the over color.

Verified in the browser preview:

- L10 for N. Sample at 28 points is 7 of 10, Wilson 40–89%. L5, L20, and the season show different rates on that same line.
- Typing 40 makes the band 0 cleared. Median restores 28. Switching to the season does not move the line.
- REB retargets the prop. The season board opens A. Ledger on the season window.
- Method says there is no odds feed and that the minutes model is not in the app.
- Desktop, 390px, and dark. The phone histogram for last 10 fits without clipping. No page errors.
- `pnpm check` and `pnpm build` are clean. `build/index.html`, `leaderboard.html`, and `method.html` are present.

Not verified inside the Tauri webview. Nothing has been committed.

Model check, after the block was added:

- A synthetic points rule of 25 at home and 15 away held out at mean absolute error 0.0 against 5.0 for the last-10 average. Sigma stayed about 1.0 because the residual prior does not collapse when the trees fit the training rows. That is a pipeline check, not an NBA result.
- `cargo test --lib`: 25 passed, 1 ignored. `pnpm check` clean.
- Preview at localhost:1420 shows the Model block, leaves Train disabled, and keeps the hit rates, chart, and home/away split. Changing opponent, site, rest, and minutes does not invent a number. Method states the leakage rule and the holdout. Phone width does not overflow.
- Train against the real cache has not been clicked. The desktop app was not opened.

## Review fixes (October 2026)

From the October code review. Branch `review-fixes-2026-10`.

- [x] Tooling: vitest (`pnpm test`), `pnpm test:math` through tsx on Node 20, `pnpm test:rust` building with cmake, nasm, and libclang.
- [x] Sync merges. An empty or short response (under half the cached games) keeps the cache and shows a warning.
- [x] One fit at a time, held by the backend. Model files are written to a temp file and renamed. The model cache cannot pin a model from before a train.
- [x] Fits are kept per stat, season, and season type. Old single-file models move to their own season on start.
- [x] 0-minute games are left out of hit rates on the board, the trend, and the player page, the same as the model. The player page and the board show the DNP count.
- [x] Rust scores all four splits, their Wilson bands, and rest days. `atLeast` stays in TypeScript so a number edit does not wait on Rust; a shared fixture pins it to the Rust copy. One stat catalog in `src/lib/catalog.json`.
- [x] Deleted `engine/features.rs` and `engine/posterior.rs`.
- [x] Boot errors show the real message. The board line, the player number, and the model spot wait 200 ms after typing. Predict reads one player's games.
- [x] CSP set in `tauri.conf.json` on `quick-fixes-2` (Adam must verify in the desktop window before merge).
- [x] Read commands moved off the main thread (`spawn_blocking`) on `quick-fixes-2`.

`cargo test --lib`: 45 passed, 1 ignored. vitest: 19 passed. `pnpm check`: 0 errors. `pnpm build` clean. Checked in the browser preview, not in the Tauri webview.

## Last-season prior (October 2026)

Branch `cross-season-prior`. Plan and backtest results in [`plan-cross-season-prior.md`](plan-cross-season-prior.md).

- [x] Each player's rate prior adds his own last season, capped at `carry_minutes` (1000) adjusted pseudo-minutes. Playoffs carry their regular season. Rookies keep the role prior. Traded players keep their carry.
- [x] Opponent multipliers start from last season's, pulled toward 1 by `opponent_carry_minutes` (1500).
- [x] Minutes start from last season's average and fade to the role as games come in.
- [x] A seeded fit borrows last season's role rates, minutes, home, and rest until this season has 100 rows, and does not wait for 20 training rows. A player with carry gets a probability before his first game.
- [x] Home has a "Carry last season" checkbox. Cards, the model page, and the player page ("Prior from" the seed season) say when a fit carried a season.
- [x] `sync-season` and `backtest-prior` bins. Backtest on the first 10 team games of 2024-25 and 2025-26.
- [x] Carry decay `exp(-m/τ)` with τ = 500. Late-season LL improved on PTS/PRA in 2024-25 and 2025-26; early buckets stayed flat.
- [x] Per-stat `carry_minutes`. All 14 stats stay at 1000; a 0–2000 grid on the other nine in 2024-25 and 2025-26 was within ~0.001 LL of 1000.
- [x] Day one. With carry on, the list adds every seed-season player and this season's rosters (`commonallplayers`, stored per season, fetched only for the season that is on or, from July, about to open). A carried player gets a probability at 0 games; a rookie is refused with the reason. The trend shows "No games this season yet" and last season's hit rate. `backtest-prior` scores each carried player's first game (`p-g1`).
- [x] Playoff list. Syncing Playoffs stores `leaguestandingsv3` clinch/play-in teams; once playoff games exist those teams win. Fallback is the wide list plus a sync note.
- [x] Mid-season roster. Every sync of the roster season refreshes `commonallplayers`. Roster overrides team and board membership; waived players stay labelled "Not on a roster".
- [x] Mid-playoff elimination. `commonplayoffseries` plus four losses in a series drops the team from the playoff list.
- [x] Read commands run off the UI thread (`spawn_blocking`); sync fetches before it locks the database.
- [x] Rookie prior from draft position: backtested on `rookie-prior`; **not shipped** (draft×role lost to role prior). Five-game wait stays. `sync-draft` + `backtest-rookie` available.
- [x] League drift: backtested; **not shipped** (mixed/tiny).
- [x] Kalshi Phase 1 (`kalshi-provider`): `OddsProvider` trait, `KalshiProvider` (public `/events` + `/markets`, cursor paging, pregame filter), `odds_snapshots` with `book = kalshi`, name + team match to NBA.com ids with unmatched names listed, `kalshi_refresh` / `kalshi_quotes`, Board column, Player ladder, edge = model P(≥X) − yes ask.
- [ ] Kalshi: confirm regular-season props once they list (week of Oct 12–19), tune the thin thresholds, and check Kalshi's team codes against NBA abbreviations.
- [ ] Kalshi polling setting (manual refresh only for now).
- [ ] Clicked through in the browser preview only, not in the Tauri webview.
