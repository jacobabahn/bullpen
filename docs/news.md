# News

Every news line is logged in `news.log` beside the tasks. `bullpen config --news`
sets how it reaches the coordinator, and `bullpen up --verbose` says which is in use
(`up` says it by itself for `monitor`):

- `type` (the default): bullpen types each line into the coordinator's pane. It
  works with any coordinator agent, and goes on working when the coordinator's
  session is restarted, since nothing has to be running inside it. But news that
  arrives while you're typing there waits (in `outbox.txt`) until the input is
  empty, so it never gets sent along with half a message: it comes late, after
  you send your own. Claude Code's dimmed prompt suggestion doesn't count as
  typing (bullpen reads the input's styling with `rex capture --format=vt`), so
  news doesn't wait behind one.
- `monitor`: nothing is typed. The coordinator runs Claude Code's Monitor tool on
  `bullpen news --follow`, and each line shows up in its chat as a notification
  without touching the input, so news never waits on your typing. It needs a
  coordinator with that tool (Claude Code), and the coordinator has to start the
  monitor and start it again every 30 minutes, when it expires; its guide tells
  it to. News logged while no monitor runs is not lost: the next one prints it.

News is for the coordinator. You hear of a task that needs you by a notification
(see [Notifications](notifications.md)).

To look at it yourself, `bullpen news --recent` prints the last 20 lines
(`--recent 50`: the last 50), oldest first, each after the time it was logged
(`3:04 PM` today, `Mon 3:04 PM` this week, `Oct 9 3:04 PM` before), with the length of
a guide's diff after its line (`(24 more lines: --json prints them)`). `--all-repos` takes the news of every repository
bullpen knows, in time order, each line with its repository's name, and works from
any folder; `--json` prints a list of `{"repo", "time", "task", "event", "to",
"text", "more"}` (`to`: the coordinators it was for, `[]` for all; `more`: a diff's
lines). It only reads `news.log`: it runs from any pane, takes no lock and moves no
coordinator's cursor, so `bullpen news` and `news --follow` print every line
afterwards just as they would have. A line still being written, and the one before
it, show a moment later.

Each line starts with the task's ID and what happened, so the start is what
counts when a narrow pane cuts it: `BP-3 is ready: Fix login redirect (2 commits,
PR #12, approved by full review)`, `BP-4 is at a permission prompt: Allow npm
install?`, ``BP-5 is working again (was ready): it must run `ready` again``. A task
ready a second time is "ready again" ("ready again after a rebase" when its base
moved), and a line word for word the last one about its task isn't sent.

`bullpen news` prints the lines logged since the last time and exits;
`bullpen news --follow` keeps running and prints each new line as it is logged.
Both remember how far they got (a byte offset in `news-c1.cursor`, one per
coordinator), so each line is printed once, across restarts too. They print the
news of the coordinator whose pane runs them (`REX_BLOCK`), or of the first one
outside Rex. From any other Rex pane they refuse, and a `--follow` running in a pane
that stops being a coordinator stops: each line is printed once, so a reader in an
agent that used to be the coordinator would take lines meant for the real one. Only
one reads per coordinator at a time: a second `--follow` waits
for the first to end and then takes over. The first run starts
from the end of the log, and switching to `monitor` starts from there too, so
old news is never printed again.

## The backstop

A monitor can lapse: it expires every 30 minutes, and the coordinator may not
start it again. So news that isn't typed has a backstop. When a line worth
acting on is logged, the watcher looks every 3 seconds (`bullpen _wake_check`,
while `<state>/wake-due` exists). Once such a line has gone a minute unread by
a coordinator whose pane is running, and no reader takes its news (a
`news --follow` holds `news-c1.lock`; a waiter, below, is named in
`news-c1.wait` while its process runs), bullpen types every line since the coordinator's cursor
into its pane, as `type` would, and moves the cursor: each line is typed once,
and `bullpen news` won't print it again. It waits behind a draft in the input,
as typed news does; if a draft holds it for 10 minutes, you get one
notification for that batch ("Your coordinator (c1) has news it hasn't seen").

Bookkeeping doesn't wake a coordinator by itself: lines for `started`,
`rebased`, `accepted`, `closed`, `queue`, `pickup`, `kept`, `dropped`, `later`,
`tour-sent` and `keys` ride along with the next line that does (or wait for
`bullpen news`). Every other line wakes, a new kind of line too.

## A waiter in the coordinator's Stop hook

`bullpen news --wake` is made to be a Claude Code hook. Bullpen doesn't install
it yet: to try it, add it yourself to the settings of the folder the coordinator
runs in, as a Stop and a StopFailure hook, with news not typed
(`bullpen config --news monitor`):

```json
{"hooks": {
  "Stop": [{"hooks": [{"type": "command", "command": "/path/to/bullpen news --wake",
                       "asyncRewake": true, "timeout": 86400}]}],
  "StopFailure": [{"hooks": [{"type": "command", "command": "/path/to/bullpen news --wake",
                              "asyncRewake": true, "timeout": 86400}]}]
}}
```

Each time the coordinator's turn ends, Claude Code starts the command in the
background, and it waits there: it looks at `news.log`'s size once a second,
starts no other program and costs no turn. When a line worth acting on is logged
for its coordinator, it waits for the batch to settle (3 seconds with no new
line, 20 at most), prints every line since the coordinator's cursor to stderr
(the bookkeeping too), moves the cursor and exits 2. Claude Code then starts a
turn in the idle session with those lines, shown as "Stop hook feedback": they
are bullpen's news, not an error and not your words. If the session is busy, the
lines come as a turn of their own once it is done.

- It knows its coordinator by the pane it runs in (`REX_BLOCK`). In any other
  pane, with typed news, or while news is paused, it exits at once: that is all
  another Claude session in the folder pays for the hook.
- Claude Code starts one per turn end and stops none, so the newest owns the
  wait (`news-c1.wait` names it) and an older one goes quietly within a second.
- A coordinator is woken once a minute at most. A batch that settles sooner
  waits out the minute.
- A turn that ends in an API error starts it too (StopFailure). After one, it
  wakes no sooner than 5 minutes after the last wake, so an outage can't loop
  error, wake, error. If the turn that failed was itself a wake, its news is said
  again then, since nobody acted on it.
- While a usage limit is in force (see [Usage limit](usage-limit.md)), it doesn't
  wake before the reset time: the turn would only fail. With no known reset
  time, it paces wakes 5 minutes apart.
- Triage's line about a task ("ended its turn without `ready` 22 / 25 / 28 min
  ago") wakes once in 15 minutes for that task; its repeats ride along with the
  next wake.
- It ends by itself a minute before the hook's timeout, since a hook killed at
  its timeout loses its exit 2. News that waits then is handed over at once; with
  none, the session has no waiter until its next turn ends, and the backstop
  covers it. For a hook with another timeout, say so: `news --wake --timeout
  3600`.
- When the session exits or the pane closes (TERM, HUP, INT) it exits without
  moving the cursor, so the next waiter, `bullpen news` or the backstop has the
  lines.
- It shares the cursor and the lock with `bullpen news` and `news --follow`, so
  no line comes twice. While a `news --follow` runs, that prints the news and
  the waiter leaves it to it. The backstop types nothing while a waiter lives.
- `bullpen news --pause` ends a parked waiter, and none starts until the resume.
- Never run `news --wake` from the coordinator's own tools: a waiter started by
  a tool call dies with that call.

## Pausing

To stop news reaching a coordinator unasked for a while, it runs
`bullpen news --pause` in its own pane (when you ask it to). The backstop then
types nothing to it and no waiter wakes it; news is still logged, and
`bullpen news` prints it. From any other pane, and outside Rex, it refuses.
`bullpen news --resume` ends the pause, and so does the next `bullpen up` for
that coordinator, with one line saying so. A pause has no timer. After a resume,
news logged during the pause gets a minute for a reader before the backstop
types it. With `type`, `--pause` refuses: there's nothing to pause.

## Switching

You can switch while a coordinator runs. Switching to `monitor` types one last
line into its pane telling it to start the monitor (not when you run `config`
from the coordinator's own pane: it sees the answer). Switching to `type` makes
a running `news --follow` print `bullpen · news is typed into your pane now` and
exit, so the coordinator stops restarting it and no line arrives twice; in `type`
mode `--follow` always does that, and plain `bullpen news` still works. News
logged in `monitor` mode but not yet read when you switch back is not typed:
`bullpen news` prints it.

## A changed guide

When the coordinator's guide changes. A coordinator gets its guide
(`prompts/coordinator.md`) once: in its system prompt, or by reading `bullpen guide`
after `up --here`, and a running agent can't load it again. So each coordinator
records the hash of the guide it was given, and the text is kept in
`~/.local/state/bullpen/guides/<hash>.md`. When `bullpen up` or `bullpen accept` (a
pull or a merge may have changed it) finds that a running coordinator's guide is
out of date, in any repository, it sends that coordinator alone one news line, and
records the new hash, so each change is told once. When the diff is short (40
lines or less) it comes after the line, as a unified diff (typed with it as one
message, or printed after it by `bullpen news`, logged as lines starting with a
tab):

```
bullpen · Your coordinator guide changed (section: News). Read the change below and follow it from now on:
--- guide (yours)
+++ guide (now)
@@ -314,4 +314,4 @@
...
```

A longer change names the sections it touches instead:

```
bullpen · Your coordinator guide changed (sections: Review, News), 63 lines of diff: run `bullpen guide` to read it all, and follow it from now on.
```

A coordinator from before this (no hash),
or one whose old text is gone, is told once that its guide may have changed, to run
`bullpen guide`, with no diff.
