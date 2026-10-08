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
