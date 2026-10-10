<header class="page-head">
  <div>
    <h1>Method</h1>
    <p>Counting rules, and the model behind the probability.</p>
  </div>
</header>

<article class="method">
  <h2>The line</h2>
  <p>
    A game clears the line when the stat is greater than or equal to it. A 26 against 26 counts.
    Combo props are box-score sums: points and assists, points and rebounds, rebounds and assists,
    and all three. The line is the number you set. Median fills it from the
    games in the window until you type a different number.
  </p>
  <p>
    A game with 0 minutes is a did-not-play. It is not a miss. The hit rates, the board, and the
    model all leave it out, so the last 10 is the last 10 games he played. The player page says how
    many were left out.
  </p>

  <h2>The hit rate</h2>
  <p>
    The prop screen shows last 5, last 10, last 20, and the season against the same line. Those four
    rates can disagree. Last 10 is the last 10 games in the cache. It is not a forecast. The band
    under the splits is a 95% Wilson interval, z = 1.96:
  </p>
  <pre>center = (p + z² / 2n) / (1 + z² / n)
margin = z √(p(1 − p) / n + z² / 4n²) / (1 + z² / n)</pre>
  <p>
    Five of the last ten is about 24% to 76%. Both ends fit the same ten games.
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
    Each counting stat is a rate per minute with a gamma prior. The percent at a line is the
    negative binomial that prior implies, summed from the line upward. It is wider when the
    expected total is higher, and it cannot go below zero. A line of 12.5 means 13 or more. The
    middle 80% of the same distribution is the usual range. For points only, that distribution
    is stretched about 15% around its middle, and a 5% chance of a short night at about a third
    of normal scoring is mixed in, because the plain version was too sure of itself against last
    season's Kalshi points prices.
  </p>
  <p>
    The rate shrinks toward players in a similar minutes role. Each opponent has a multiplier fit
    with those rates. Home and an extra day of rest are two more multipliers, with tight priors. A
    blank opponent is the league average.
  </p>
  <p>
    Last 5, last 10, and last 20 each carry a second rate. "New rate" is the weight on that rate
    mixed with the season rate. A low weight stays close to the season rate. The season window does
    not run the test. The weight stays at zero until there are five games before the window. Combos
    do not show one number, because the parts can disagree.
  </p>
  <p>
    Minutes are a separate distribution: the last 10, shrunk toward an anchor with a weight of
    three games, drawn as a lognormal from 0 to 48. The anchor is the minutes role. With last
    season carried, it starts at last season's minutes per game and moves toward the role. Points,
    rebounds, and assists share that distribution. Given the minutes, the rates stay separate.
    Typing minutes uses that number, up to 48. A combo is the sum of its parts. Home and rest still
    apply to each part. The model page does not show one home or rest multiplier for a combo.
  </p>
  <p>
    The last 20% of dates are held out. The saved model is the one that did not see those dates, so
    the error on the model page is the error of the model you are looking at. That error sits next
    to the player's last-10 total on the same games. If the average was closer, both numbers still
    show.
  </p>
  <p>
    A season starts from the one before it when that season is cached. Playoffs start from their
    regular season. Each player's rate prior adds his own totals from last season, adjusted for
    opponents, home, and rest, and capped at 1,000 pseudo-minutes. The weight on that carry is
    <code>exp(-m / 500)</code>, where m is minutes played this season, so it is about 37% after 500
    minutes. The player page says "Prior from" the seed season while the stored carry, before that
    decay, is still larger than this season's adjusted minutes. For a starter that is about 30
    games. His minutes start from last season's average, counted as three games. Each opponent
    multiplier starts from last season's, pulled toward 1. A rookie keeps the role prior and waits
    for five games. A traded player keeps his own carry. A player with carry gets a probability
    before his first game. Everyone else waits for five. Untick "Carry last season" on the home
    page to fit without it.
  </p>
  <p>
    Before the first game, with carry on, the list adds everyone who played in the seed season. A
    sync of the current season, or from July the season about to open, refreshes the roster. A
    traded player's team is the roster team. A player with games who is on no roster stays listed
    as "Not on a roster", and the board leaves him out. A carried player with no games who is off
    the roster is dropped. If the roster call fails, or returns fewer than 300 players, the stored
    roster is kept. With no roster, the page says "Team from" the seed season. Playoffs keep
    clinched and play-in teams until playoff games are cached, and then keep the teams in those
    games. A team with four losses in a series drops out. With no games yet, the trend says so and
    shows last season's hit rate on the same number.
  </p>
  <p>
    The spec for each stat is <code>models/specs</code>: prior minutes, opponent shrinkage, the
    shift prior, the role cuts, the home and rest priors, the two carry caps, and the carry decay.
    An edit takes effect the next time you train. Training needs at least 20 rows after the
    holdout, unless it carries last season. Fitted models stay in the app data folder. There is no
    schedule feed. The opponent, the site, the rest, and the minutes are the spot you name.
    Training runs in the desktop app.
  </p>

  <h2>What was left behind</h2>
  <p>
    An earlier version drew one minute total and scaled points, rebounds, and assists by that same
    number. The rates here are separate once the minutes are known. The shared piece is only the
    minutes. An earlier fit was a tree. A file from that fit will not load. Refit the season on the
    home page. The only price in the app is Kalshi's public prop market. A Kalshi rung such as 25+
    points is a yes/no contract, so its ask is a probability. The edge shown is the model's chance of
    that many or more minus the yes ask, before Kalshi's fee, and the mid is shown as the market's own
    estimate. Thin rungs are greyed out. There is no Kalshi market for turnovers, field goals made, or
    field goals attempted.
  </p>

  <h2>The rows</h2>
  <p>
    Sync asks <code>stats.nba.com/stats/playergamelogs</code> for one season type and stores the
    box score in SQLite on this machine. The host drops ordinary HTTP clients, so the app speaks
    with a Chrome TLS fingerprint. All-Star games, whose ids start with 003, are left out.
  </p>
</article>
