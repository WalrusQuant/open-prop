# Lessons

## The screen is a prop board

The first UI copied the Shiny desk: an editorial title, one window, and a chart that never said who the prop was. That read as a bad port from R.

Build the page the way a prop tool is read. The player is the title. The stat is a chip. One line stays put while L5, L10, L20, and the season show their hit rates next to each other. The chart is those games against the line. The Wilson interval sits on the count, because a short sample is wide.

Do not add an odds feed, a price, or an edge. Do not bring back the minutes draw that scaled points, rebounds, and assists together. Do not resample the same games and call it a simulation. Use the box score for a split the count does not already show, such as home, away, and rest.

The prediction is a separate model, and the trend screen stays. Each counting stat is a rate per minute with a gamma prior, shrunk toward a minutes role, plus an opponent multiplier and small home and rest multipliers. The spread is the negative-binomial posterior predictive, not a shared normal residual. Last 5, last 10, and last 20 mix in a second rate only when a Bayes factor says the window is a real shift. Combos are sums of those parts. Points, rebounds, and assists share minutes uncertainty, and the rates stay separate once minutes are fixed. Do not bring back the gradient booster, and do not replace this with a redraw of the window. Do not delete the hit rates to make room for it.

Home is the cache and the fitted models: what is loaded, sync, when each stat was last fit, and a link into that stat's test. The player page is a step in from there. It keeps the trend, the number being checked, and the probability, and it shows the model's distribution next to that window's average. Say what each number is in a sentence: the percent is the chance of that many or more, the model number is what the model expects, and the recent games are what he actually averaged. Do not label the input "Line" or the result "projection" and leave it at that. Do not put holdout error on the player page. Do not put the player prop back on `/`. Do not delete Board or Method.

Plain sentences in one left-hand stack still failed. The number being checked has to be its own control, and the model and the trend have to be separate cards. Opponent, home or away, rest, and minutes belong inside the model card, at their own width. A later pass made that number and the percent enormous and laid the model facts in one overflowing row, so the labels sat on top of the values and the cards were mostly empty. Keep one type size. Put each fact in its own column with the label above the value. Do not leave a wide empty card around a single input.

The board is not a season-average ranking of the whole league. Default it to who cleared the line in the last 10, and keep a search for everyone else.
