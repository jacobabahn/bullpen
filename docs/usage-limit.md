# Usage limit

All of bullpen's agents share one Claude subscription. When it runs out, a
worker or reviewer stops with a line like `You've hit your 5-hour limit ·
resets 3pm`. Bullpen reads that line from the status the agent reports (or,
for a turn that failed or went idle, from the end of its screen) and:

- keeps the task where it was, working or in review, marked "Usage limit, resets
  3:00 PM" on the board and in `ls`. It doesn't need you;
- posts one news line (`BP-3 stopped at the usage limit, resets 3:00 PM: nothing
  to do, it is continued then`);
- makes `bullpen start` refuse, saying why, until the reset time or until a
  limited agent is working again. `bullpen start BP-4 --force` starts anyway.

Two minutes after the reset time the watcher continues each limited task: it
tells the agent to carry on, or resumes the worker (starts the reviewer again)
if its pane is gone, and posts one news line (`the usage limit has reset:
continued BP-3, BP-5's reviewer`). The two minutes leave room for Claude Code's
own "Continue automatically at usage limit" setting; an agent that is working
again by then is left alone.

If the reset time can't be read from the message, the news says so and the task
isn't continued by itself: `bullpen tell BP-3 continue` once the limit has
reset. `start` then refuses for 5 hours, or until that agent works again. The
limit is kept in `limit.json` in the state folder, for every repository.
