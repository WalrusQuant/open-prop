# Plan: last-season prior

## Goal

Per-season fits leave the first week or two of a season dark. Training needs 20 scored rows (games
with five earlier games for that player), and a prediction waits for five cached games per player.
Carry each player's last season into this season's prior, so the model can price a player after
his first game and leans on this season more as the minutes pile up.

Nothing in `tasks/lessons.md` changes. Each counting stat is still a gamma prior on a rate per
minute, shrunk toward a minutes role, with opponent, home, and rest multipliers, a Bayes-factor
window shift, and a negative-binomial posterior predictive. No booster, no window redraw, and the
hit rates stay.

## Math

Today a player's rate prior is `Gamma(prior_minutes × role_rate, prior_minutes)`, and his
season adds `Σ stat` to the shape and `Σ adjusted minutes` to the rate.

**Player seed: a capped power prior on last season, on top of the role prior.** For season S,
take the player's season S−1 totals for the stat: `Y` (stat) and `M` (minutes adjusted by S−1's
own opponent, home, and rest multipliers). Discount that likelihood by
`a = min(1, carry_minutes / M)` and add it to the role prior:

```
shape₀ = prior_minutes × role_rate_S + a × Y
rate₀  = prior_minutes + a × M
```

- The prior mean is a blend of the role rate and his own last-season rate, weighted
  `prior_minutes : min(carry_minutes, M)`. At most `carry_minutes` pseudo-minutes come over, and a
  player who barely played last season brings only what he played.
- The role prior stays underneath as the fallback. A rookie, or anyone with no S−1 rows, has
  `a × Y = a × M = 0` and gets today's prior unchanged.
- This option beats "shrink the carry-over mean toward the role" because it is one conjugate
  update with one knob. It is a standard power prior (a likelihood raised to a power at most 1),
  so nothing new enters the posterior predictive. It stays negative binomial.
- The same prior feeds the window shift test, so a traded player whose new role shows up in his
  recent games still trips the Bayes factor.

**Minutes.** For a seeded player, the minutes anchor starts at his S−1 minutes per game instead of
the role mean, and fades to the role mean as this season's games come in:
`anchor = (3 × m₋₁ + n × role_mean) / (3 + n)`. The minutes mean is still
`(Σ last 10 + 3 × anchor) / (n₁₀ + 3)`. His role for the rate prior uses the same three-game blend,
`(Σ minutes + 3 × m₋₁) / (n + 3)`. A first version kept the S−1 anchor for the whole season; it
cost about 0.04 points of holdout MAE in March and April, and the fade removed most of that while
also scoring better early (see Results).

**Opponents.** Today `f = (seen + k) / (expected + k)`, with `k = opponent_minutes × league_rate`.
With a seed, the S−1 multiplier `f₋₁` enters as a second pseudo-observation:

```
f = (seen + k + k_c × f₋₁) / (expected + k + k_c),   k_c = opponent_carry_minutes × league_rate
```

With no S games against a team, this is `(k + k_c f₋₁) / (k + k_c)`: last season's multiplier,
shrunk toward 1 harder than it was fit. A team with no S−1 multiplier starts at 1.

**Population.** Role rates, role minutes, and the home and rest terms are fit on season S once it
has 100 or more training rows for the stat. Before that, a seeded fit borrows them from the S−1
population. An unseeded fit behaves exactly as today.

**Sources.** A Regular Season fit seeds from the previous season's Regular Season games. A
Playoffs fit seeds from the same season's Regular Season. The seed is computed from the cached
games, not from a saved model file, so it cannot go stale or mismatch a spec. If the source season
is not cached, the fit is unseeded and says so.

**Gates.** A seeded fit may save with fewer than 20 scored rows (the holdout numbers are blank
until there are enough). A player with carry-over can be priced from his first game. Players
without carry-over keep the five-game rule.

## Data and settings

- `carry_minutes` per stat spec (0 to 5000, 0 turns the player seed off), default from the backtest.
- `opponent_carry_minutes` per stat spec (0 to 20000), default 1500, so `f₋₁` lands halfway to 1.
- Each player's carry (discounted `Y`, `M`, minutes per game) is stored in the model file, and
  the model records `seeded_from` ("2025-26 Regular Season").
- No schema change. The source season is just another cached season.

## UI

- Home: a "Carry last season" checkbox above the cards, on by default. It is disabled with a hint
  when the source season is not cached. Cards show "Carried 2024-25" when a fit was seeded, and a
  refit that asked for a carry it could not find says so.
- Model page: the seed source plus the two carry priors in Settings.
- Player page: while a player's carry pseudo-minutes are more than this season's adjusted minutes,
  the model card says "Prior from 2025-26", so it is clear the number leans on last season.
- Method and README: one paragraph each.

## Backtest

- Data: `sync-season` (new bin, the app's own client and merge) pulls 2023-24, 2024-25, and
  2025-26 Regular Season into a scratch database outside the repo.
  One request per season type.
- `backtest-prior` (new bin) walks each game date until every team has played 10. For each date,
  it fits on games before that date only (no holdout), then predicts every player-game that day
  with blank minutes, the real opponent, home or away, and rest. That is the default spot on the
  player page.
- Arms: no player seed (`carry_minutes` 0) against 100, 250, 500, 1000, 2000, and 4000. Opponent
  carry (0, 750, 1500, 3000, 6000) is checked separately at the chosen default. A second run to
  team game 30 checks that the carry does not hurt once the season has data.
- Scores: log loss and CRPS on the NB predictive, and Brier score of P(stat ≥ line) at common lines.
  Stats: points, rebounds, assists, threes, and PRA. Bucketed by the team's game number: 1–3, 4–6,
  7–10.
- The baseline uses the same population fallback, so it scores the role prior alone. The app today
  prices none of these games before a player has five, so the report also prints how many
  player-games the current gates would cover.

## Risks

- League-wide drift (pace, scoring) moves every rate a little between seasons. The role prior is
  refit on S as soon as there are 100 rows, and the carry is capped, so drift only lives in the
  carried part.
- A big role change (trade, injury return) starts from the wrong rate. The cap keeps it to
  `carry_minutes`, the season adds evidence every game, and the shift test still runs.
- Model files grow by one small record per player per part: about 50 KB for a single stat and
  150 KB for PRA, up from 2 to 5 KB.
- The backtest picks one default for every stat from points, rebounds, assists, threes, and PRA.

## Results

Run on 2025-26 seeded from 2024-25, and 2024-25 seeded from 2023-24, Regular Season only. Every
player-game in a team's first 10 games is predicted from games before its date, with blank minutes
(the minutes distribution), the real opponent, site, and rest, window last 10. ECE is the expected
calibration error of P(stat ≥ line) over ten probability bins. Lines: PTS 10.5/15.5/20.5/25.5,
REB 4.5/6.5/8.5, AST 2.5/4.5/6.5, 3PM 1.5/2.5, PRA 20.5/30.5.

**2025-26 seeded from 2024-25, team games 1-10.** Role prior only (carry 0) vs carry 1000. Log loss is nats per player-game, lower is better.

| Stat | Arm | LL g1-3 | LL g4-6 | LL g7-10 | LL all | LL on rows the app prices today | CRPS | Brier | ECE |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| PTS | 0 | 4.647 | 3.250 | 3.259 | 3.674 | 3.308 | 3.859 | 0.1167 | 0.0593 |
| PTS | 1000 | 3.307 | 3.140 | 3.173 | 3.204 | 3.234 | 3.262 | 0.0981 | 0.0297 |
| REB | 0 | 2.638 | 2.255 | 2.232 | 2.361 | 2.293 | 1.527 | 0.1309 | 0.0397 |
| REB | 1000 | 2.167 | 2.134 | 2.149 | 2.150 | 2.210 | 1.290 | 0.1095 | 0.0156 |
| AST | 0 | 2.126 | 1.855 | 1.804 | 1.916 | 1.892 | 1.046 | 0.1144 | 0.0411 |
| AST | 1000 | 1.728 | 1.763 | 1.736 | 1.741 | 1.830 | 0.918 | 0.0993 | 0.0121 |
| 3PM | 0 | 1.563 | 1.355 | 1.354 | 1.417 | 1.444 | 0.632 | 0.1532 | 0.0410 |
| 3PM | 1000 | 1.305 | 1.288 | 1.294 | 1.295 | 1.394 | 0.569 | 0.1410 | 0.0178 |
| PRA | 0 | 5.038 | 3.602 | 3.526 | 4.004 | 3.553 | 5.247 | 0.1258 | 0.0651 |
| PRA | 1000 | 3.613 | 3.473 | 3.444 | 3.504 | 3.477 | 4.314 | 0.1041 | 0.0251 |

3,359 player-games per stat. The app today can price 39% of them (five earlier games).

**2024-25 seeded from 2023-24, team games 1-10.** Role prior only (carry 0) vs carry 1000. Log loss is nats per player-game, lower is better.

| Stat | Arm | LL g1-3 | LL g4-6 | LL g7-10 | LL all | LL on rows the app prices today | CRPS | Brier | ECE |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| PTS | 0 | 4.413 | 3.395 | 3.246 | 3.652 | 3.318 | 3.835 | 0.1171 | 0.0670 |
| PTS | 1000 | 3.138 | 3.272 | 3.167 | 3.189 | 3.252 | 3.217 | 0.0974 | 0.0363 |
| REB | 0 | 2.630 | 2.274 | 2.195 | 2.353 | 2.286 | 1.533 | 0.1324 | 0.0455 |
| REB | 1000 | 2.096 | 2.154 | 2.142 | 2.131 | 2.221 | 1.283 | 0.1078 | 0.0136 |
| AST | 0 | 2.102 | 1.834 | 1.756 | 1.886 | 1.879 | 1.026 | 0.1094 | 0.0385 |
| AST | 1000 | 1.660 | 1.729 | 1.710 | 1.700 | 1.819 | 0.875 | 0.0917 | 0.0089 |
| 3PM | 0 | 1.553 | 1.403 | 1.349 | 1.428 | 1.443 | 0.650 | 0.1512 | 0.0381 |
| 3PM | 1000 | 1.267 | 1.326 | 1.298 | 1.297 | 1.387 | 0.581 | 0.1355 | 0.0145 |
| PRA | 0 | 4.793 | 3.690 | 3.521 | 3.966 | 3.570 | 5.174 | 0.1214 | 0.0648 |
| PRA | 1000 | 3.381 | 3.541 | 3.445 | 3.454 | 3.502 | 4.148 | 0.0965 | 0.0230 |

3,312 player-games per stat. The app today can price 40% of them (five earlier games).

**Carry grid.** Log loss over all of games 1-10, summed over the five stats, opponent carry 1500.

| carry_minutes | 2025-26 | 2024-25 |
|---:|---:|---:|
| 0 | 13.3713 | 13.2858 |
| 100 | 12.0588 | 11.9546 |
| 250 | 11.9616 | 11.8500 |
| 500 | 11.9129 | 11.7937 |
| 1000 | 11.8938 | 11.7714 |
| 2000 | 11.8946 | 11.7681 |
| 4000 | 11.8951 | 11.7680 |

**Opponent carry.** Same sum, carry 1000.

| opponent_carry_minutes | 2025-26 | 2024-25 |
|---:|---:|---:|
| 0 | 11.8952 | 11.7752 |
| 750 | 11.8939 | 11.7722 |
| 1500 | 11.8938 | 11.7714 |
| 3000 | 11.8944 | 11.7713 |
| 6000 | 11.8958 | 11.7723 |

**Team games 11-30.** Log loss, carry 0 vs 1000.

| Stat | 2025-26 carry 0 | 2025-26 carry 1000 | 2024-25 carry 0 | 2024-25 carry 1000 |
|---|---:|---:|---:|---:|
| PTS | 3.240 | 3.218 | 3.235 | 3.207 |
| REB | 2.203 | 2.172 | 2.210 | 2.178 |
| AST | 1.779 | 1.756 | 1.773 | 1.748 |
| 3PM | 1.357 | 1.319 | 1.365 | 1.334 |
| PRA | 3.535 | 3.510 | 3.528 | 3.493 |

### Reading

- The carry wins every stat, both seasons, every bucket, and every score (log loss, CRPS, Brier,
  ECE). The gain is largest in team games 1–3 (points log loss 4.65 → 3.31 in 2025-26), where the
  role prior alone has no idea who is a starter.
- It also wins on the 39–40% of rows the app can already price, and in team games 11–30.
- Calibration of P(stat ≥ line) improves by about half to two thirds (points ECE 0.059 → 0.030).
- Between 500 and 4000 the sum barely moves; 100 already captures most of the gain because the
  carried minutes anchor does much of the work.
- Opponent carry is flat to the fourth decimal. 1500 is at or next to the best in both seasons, so
  the planned default stays.

### Default

`carry_minutes = 1000` for every stat. It is at the top of the flat region in both seasons (2000 and
4000 are within 0.0035 summed log loss), and the smaller cap keeps less stale information late in
the season and has slightly better calibration than 2000 and 4000. `opponent_carry_minutes = 1500`.

### End-of-season check

`train-season` on the full 2025-26 Regular Season (holdout is the last 20% of dates, March and
April) with and without the 2024-25 seed, after the minutes fade:

| Stat | Holdout MAE, no carry | Holdout MAE, carry 1000 |
|---|---:|---:|
| PTS | 4.88 | 4.90 |
| REB | 1.89 | 1.88 |
| AST | 1.41 | 1.41 |
| 3PM | 0.93 | 0.92 |
| PRA | 6.43 | 6.45 |

The late season is a wash (within 0.02). The carry is for the opening weeks; a time decay on the
rate carry could close the last 0.02 on points and PRA and is left as an open question.

### Rerun

```bash
cd src-tauri
cargo run --release --bin sync-season -- /tmp/backtest.db 2023-24
cargo run --release --bin sync-season -- /tmp/backtest.db 2024-25
cargo run --release --bin sync-season -- /tmp/backtest.db 2025-26
cargo run --release --bin backtest-prior -- /tmp/backtest.db 2025-26 2024-25 0,100,250,500,1000,2000,4000 1500 10
cargo run --release --bin backtest-prior -- /tmp/backtest.db 2025-26 2024-25 1000 0,750,1500,3000,6000 10
```

Each `backtest-prior` run takes about 10 seconds for five stats in release mode.
