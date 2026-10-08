<header class="page-head">
  <div>
    <h1>Method</h1>
    <p>What the prop screen counts, and what it refuses to invent.</p>
  </div>
</header>

<article class="method">
  <h2>The line</h2>
  <p>
    A game clears the line when the stat is greater than or equal to it. A 26 against 26 counts.
    Combo props are box-score sums: points and assists, points and rebounds, rebounds and assists,
    and all three. There is no odds feed, so the line is yours. Median fills it from the games in
    the window until you type a different number.
  </p>
  <p>
    A game with 0 minutes is a did-not-play. It is not a miss. The hit rates, the board, and the
    model all leave it out, so the last 10 is the last 10 games he played. The player page says how
    many were left out.
  </p>

  <h2>The hit rate</h2>
  <p>
    The prop screen shows last 5, last 10, last 20, and the season against the same line. Those four
    rates can disagree. Last 10 is the last 10 games in the cache, not a projection of the next one.
    The band under the splits is a 95% Wilson interval, z = 1.96:
  </p>
  <pre>center = (p + z² / 2n) / (1 + z² / n)
margin = z √(p(1 − p) / n + z² / 4n²) / (1 + z² / n)</pre>
  <p>
    Five of the last ten is about 24% to 76%. Both ends fit the same ten games. That width is the
    result. The percentage in the middle is just where the count landed.
  </p>

  <h2>The chart</h2>
  <p>
    Green cleared the line. Red missed it. The dashed mark is the line. The gold path is the mean
    of the current game and the two before it, inside the window only. The first two games have no
    average. It describes the window. It does not forecast the next game.
  </p>

  <h2>The same games, split</h2>
  <p>
    Home and away come from the matchup. Back-to-back means the previous game in this cache was
    the day before. The other games are rested. These are the same games as the chart, counted
    again. Two of three is still two games.
  </p>

  <h2>The model</h2>
  <p>
    Each counting stat is a rate per minute. The rate shrinks toward players in a similar minutes
    role, so five loud games cannot invent a new player. Each opponent has a multiplier fit with
    those rates, so a big night against a soft defense does not all stick to the player. Home and
    an extra day of rest are two more multipliers, with tight priors, so they stay small unless the
    games support them. A blank opponent is the league average. The fit is plain Rust. There is no
    tree library and no sampler.
  </p>
  <p>
    The predictive count is wider when the expected total is higher, and it cannot go below zero.
    The percent at a line is that distribution, summed from the line upward. A line of 12.5 means
    13 or more. The middle 80% of the same distribution is the usual range. It is not a redraw of
    old games.
  </p>
  <p>
    The trend window is part of the model. Last 5, last 10, and last 20 each get a second rate and
    a probability that those games are a real change rather than noise. If that probability is low,
    the prediction stays with the season rate. The season window does not run that test. On the
    player page that probability is the "New rate" figure. Combos do not show one, because the
    parts can disagree.
  </p>
  <p>
    Minutes have their own distribution: the last 10, shrunk toward the role, and clipped from 0 to
    48. Points, rebounds, and assists share that uncertainty. Given the minutes, the rates stay
    separate. Typing minutes removes the shared piece. A combo is the sum of its parts, not its own
    rate.
  </p>
  <p>
    The last 20% of dates are held out. The saved model is the one that did not see those dates, so
    the error on the model page is the error of the model you are looking at. That error sits next
    to the player's last-10 total on the same games. If the average was closer, both numbers still
    show. There is no odds feed and no edge against a book.
  </p>
  <p>
    A season starts from the one before it when that season is cached. Playoffs start from their
    regular season. Each player's rate prior adds his own totals from last season, adjusted for the
    opponents, home, and rest, and capped at 1,000 pseudo-minutes. A starter's own games outweigh
    that after about 30 games, and the carry also fades as this season's minutes grow. Until then the player page says "Prior from" that season. His
    minutes start from last season's average, counted as three games. Each opponent multiplier
    starts from last season's, pulled toward 1. A rookie keeps the role prior and waits for five games; a draft×role prior was tried and did not beat that. A traded player keeps
    his own carry, because the multipliers belong to opponents, not teammates. A player with carry
    can be priced before his first game. Everyone else waits for five. The cap came from a backtest
    on the first 10 team games of the last two seasons, where every game was predicted from earlier
    games only. Untick "Carry last season" on the home page to fit without it.
  </p>
  <p>
    Before opening night the player list is this season's rosters and everyone from last season.
    A sync of the current season asks stats.nba.com for the rosters again each time, so a traded
    player's team is the roster team and a waived player stays listed as "Not on a roster" (the
    board leaves him out). If that call fails, the stored roster is kept. Playoffs ask
    leaguestandingsv3 for clinched and play-in teams; once playoff games are cached those teams
    are the list. With no games yet, the trend says so and shows last season's hit rate, labelled.
    On each carried player's first game of 2025-26, the carry cut points log loss from 5.83 to
    3.31 against the role prior alone.
  </p>
  <p>
    The spec for each stat is <code>models/specs</code> in the source tree: prior minutes, opponent
    shrinkage, the shift prior, the role cuts, the home and rest priors, and the two carry caps. An edit takes effect
    the next time you train. Training needs at least 20 rows after the holdout, unless it carries
    last season. Fitted models stay
    in the app data folder. They are not part of the source tree. A file saved by the old tree
    model will not load. Refit the season on the home page. There is no schedule feed. The
    opponent, the site, the rest, and the minutes are the spot you name. Training runs in the
    desktop app.
  </p>

  <h2>What was left behind</h2>
  <p>
    An earlier version drew one minute total and scaled points, rebounds, and assists by that same
    number. The counting stats moved in lockstep. This model is not that draw. The rates are
    separate once the minutes are known. The shared piece is only the minutes. Betting odds,
    implied probability, and an edge against a book are not in this app. Neither is the tree model
    that used to estimate each stat.
  </p>

  <h2>The rows</h2>
  <p>
    Sync asks <code>stats.nba.com/stats/playergamelogs</code> for one season type and stores the
    box score in SQLite on this machine. The host drops ordinary HTTP clients, so the app speaks
    with a Chrome TLS fingerprint. All-Star games, whose ids start with 003, are left out.
  </p>
</article>
