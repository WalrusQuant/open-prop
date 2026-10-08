# Open Prop

Open Prop is a desktop app for NBA game logs. It shows how often a player cleared a number you pick, and it fits one rate-per-minute model per stat for the season. The model is there so you can read a probability and change the priors. It does not claim an edge against a book.

There is no odds feed, no American price, no implied probability, and no edge. A line is just the number you typed.

[Watch a one-minute demo](docs/demo.mp4).

The app is a [Tauri](https://tauri.app/) shell. Rust owns the cache, the sync, and the model. The screens are Svelte. Fitted models and the database stay on your machine. They are not part of this repository.

## Screens

### Home

![Home tab. The cache, the fitted models, and the player search.](docs/home.png)

Home is the cache and the models. It shows how many games are stored, when they were synced, and when each stat was last fit. Sync and refit live here. Open a stat for that model's priors and its holdout test, or open a player.

### Player

![Player tab. One player, the number, the model, and the trend.](docs/player.png)

The player page is one player and one stat. You set the number being checked. Last 5, last 10, last 20, and the season are scored against that same number. The model card is the chance of that many or more for a spot you name: opponent, home or away, days of rest, and minutes. Leave the opponent blank for a league-average opponent. Leave minutes blank and the model uses a minutes distribution. Type minutes and that distribution collapses to the number you typed. The distribution chart draws the model's probabilities from zero up, next to the games in the selected window. Changing the number sums a tail that is already on the page. It does not ask the model again. Changing the player, the stat, the window, or the spot does.

The board is the short list. Pick a stat and a number. It keeps players who cleared that number in at least 70% of their last 10, with at least 8 games in that window and 10 games on the season. You can drop the floor to 60% or 80%, or show everyone and search. A row opens that player.

Method is the counting rules and the model, written out. It is not a second board.

## What a game counts as

A game clears the number when the stat is greater than or equal to it. A 26 against 26 counts. A line of 12.5 on the model means 13 or more, because the predictive is a count.

| Stat | Id | Default number |
| --- | --- | --- |
| Points | `points` | 20 |
| Rebounds | `rebounds` | 6 |
| Assists | `assists` | 5 |
| Steals | `steals` | 1 |
| Blocks | `blocks` | 1 |
| Turnovers | `turnovers` | 2 |
| Field goals made | `field_goals_made` | 7 |
| Field goals attempted | `field_goals_attempted` | 15 |
| Threes made | `three_point_field_goals_made` | 2 |
| Free throws made | `free_throws_made` | 4 |
| Points + assists | `points_assists` | 25 |
| Points + rebounds | `points_rebounds` | 25 |
| Rebounds + assists | `assists_rebounds` | 10 |
| Points + rebounds + assists | `points_assists_rebounds` | 30 |

Combo stats are box-score sums. The player page fills the number from the median of the selected window until you type one. Changing the window does not move a number you already set.

The band under the hit rates is a 95% Wilson interval, z = 1.96. Five of the last ten is about 24% to 76%. The width is the result. The percentage in the middle is just where the count landed.

The chart marks a game green when it cleared and red when it missed. The gold path is the current game plus the two before it, inside the window only. The first two games have no average. That path describes the window. It does not forecast the next game.

Home and away come from the matchup text (`DAL vs. CHI` is home, `DEN @ SAS` is away). Back-to-back means the previous game in this cache was the day before. The other games are rested. These are the same games, counted again.

The player page can export the games in the window as CSV.

## The model

Train once per stat per season, from the home page. Every counting stat is a rate per minute. A short sample shrinks toward players in a similar minutes role, so five loud games cannot invent a new player. The shipped cuts are under 15 minutes, 15 to 28, and 28 or more.

Each opponent has its own multiplier, fit with the rates, so a big night against a soft defense does not all stick to the player. Home and an extra day of rest are two more multipliers. Their priors are tight, so the effects stay small unless the games support them. A blank opponent is a multiplier of 1. An opponent abbreviation that never appeared in the cache is an error.

The predictive count gets wider as the expected total gets higher, and it cannot go below zero. Blocks and steals keep their zeros in that same distribution. The percent at a line is that distribution, summed from the line up. The usual range is the middle 80% of it.

The trend window is part of the model. Last 5, last 10, and last 20 each get a second rate and a probability that those games are a real change rather than noise. If that probability is low, the prediction stays with the season rate. On the player page that probability is "New rate". The season window does not run the test. Combos do not show one number for it, because the parts can disagree.

Minutes have their own distribution: the last 10, shrunk toward the role, clipped from 0 to 48. Points, rebounds, and assists share that uncertainty, and so do the combos built from them. Given the minutes, the rates are separate. This is not a draw that scales every stat by one minute shock.

A combo is the sum of its parts. Points + assists is the points model plus the assists model. The combo file stores those fitted parts, so a prediction does not depend on load order. Home and rest are left off the combo page because the parts do not share one multiplier.

The last 20% of distinct dates are held out. The saved model is the one that did not see those dates. The model page shows that model's mean absolute error next to the player's own last-10 total on the same games, and how often the actual stat landed in the middle 80%. If the last-10 total was closer, both numbers still show. Holdout error stays on the model page. It is not on the player page.

A prediction waits until the player has five cached games. The first game of a season uses a neutral rest of 2 days, because there is no previous day in the cache. Later rest is the gap between games, capped at 14. There is no schedule feed. The opponent, the site, the rest, and the minutes are the spot you name. They are not "tomorrow".

Points are not literally a Poisson process. A point total is twos and threes. The working model is still a count per minute. The box score has no three-point attempts and no free-throw attempts, so the app does not invent a shot-level model.

An older tree file will not load. The home page asks you to refit.

## Changing the model

Each stat has a spec in [`models/specs`](models/specs). The app reads the first file it finds, in this order: `models/specs` from the working directory, `../models/specs`, `specs` in the app data folder, then the copy compiled into the binary. An edit takes effect the next time you refit.

Shipped values, same for every stat:

| Field | Shipped | What it does | Allowed |
| --- | --- | --- | --- |
| `prior_minutes` | 240 | Pseudo-minutes in the role prior. Larger shrinks a short sample harder. | (0, 5000] |
| `opponent_minutes` | 1500 | Pseudo-minutes that shrink an opponent multiplier toward 1. | (0, 20000] |
| `shift_prior` | 0.1 | Prior probability that the recent window is a new rate. | (0, 1) |
| `role_minutes` | `[15, 28]` | Rising cuts, in minutes, that separate roles. | 1 to 4 cuts, each in (0, 48) |
| `home_sd` | 0.08 | Prior standard deviation of the log home multiplier. | (0, 1] |
| `rest_sd` | 0.015 | Prior standard deviation of the log rest multiplier, per day. | (0, 0.2] |

Training needs at least 20 rows after the holdout. A row is a game that already has five earlier games for that player. Games with zero minutes are skipped. The game being scored is not inside its own rate.

## Data

Sync asks `stats.nba.com/stats/playergamelogs` for one season type and stores the box score in SQLite on this machine. The host drops ordinary HTTP clients, so the app speaks with a Chrome TLS fingerprint. Retries are limited. All-Star games, whose ids start with `003`, are dropped. Numeric game ids are padded back to 10 digits before that check, because a number would lose the leading zeros.

Regular Season and Playoffs are separate syncs. The season label looks like `2025-26`. Before October 22 the suggested season is the one that just finished. On October 22 it moves to the season that is opening.

Stored columns are season, season type, player id, player name, team, game id, date, matchup, win or loss, minutes, points, rebounds, assists, steals, blocks, turnovers, field goals made and attempted, threes made, free throws made, and plus-minus. There are no offensive rebounds, no three-point attempts, no free-throw attempts, no position, and no schedule.

The database uses WAL mode. On macOS it lives at:

```text
~/Library/Application Support/com.openprop.desk/open-prop.db
```

Fitted models are JSON files in the `models` folder next to that database. One file per stat. The browser preview at `pnpm dev` uses invented names. It does not call the NBA API, and it cannot train.

## Run

Requirements: Node 22 or newer, pnpm, Rust 1.85 or newer, and `cmake` plus `nasm`. The HTTP client builds BoringSSL, and those two tools are what that build needs.

```bash
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` is the desktop app. The window talks to a Vite server on port 1420.

You can also refit from the cache without opening the window. The default database path below is the macOS one.

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin train-season -- \
  "$HOME/Library/Application Support/com.openprop.desk/open-prop.db" \
  2025-26 "Regular Season"
```

The command prints holdout error, last-10 error, 80% coverage, and the row counts. Pass another database path, season, or season type as the three arguments.

## Tests

```bash
pnpm check          # Svelte and TypeScript
pnpm test:math      # Wilson interval and the small desk helpers
pnpm test:rust      # season calendar, parser, cache, model
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored live_regular_season
```

The ignored test downloads a regular season. It is the check that the client still gets through.

## Layout

```text
src/                  SvelteKit screens
src-tauri/src/        Rust commands, cache, sync, and the model
models/specs/         Priors, one JSON file per stat
scripts/              The small math check
```

## License

Apache License 2.0. Copyright 2026 Adam Wickwire. See [LICENSE](LICENSE).
