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

A monitor can lapse: it expires every 30 minutes, and the coordinator may not
start it again. So news that isn't typed has a backstop. When a line worth
acting on is logged, the watcher looks every 3 seconds (`bullpen _wake_check`,
while `<state>/wake-due` exists). Once such a line has gone a minute unread by
a coordinator whose pane is running, and no `news --follow` reads for it (one
holds `news-c1.lock`), bullpen types every line since the coordinator's cursor
into its pane, as `type` would, and moves the cursor: each line is typed once,
and `bullpen news` won't print it again. It waits behind a draft in the input,
as typed news does; if a draft holds it for 10 minutes, you get one
notification for that batch ("Your coordinator (c1) has news it hasn't seen").

Bookkeeping doesn't wake a coordinator by itself: lines for `started`,
`rebased`, `accepted`, `closed`, `queue`, `pickup`, `kept`, `dropped`, `later`,
`tour-sent` and `keys` ride along with the next line that does (or wait for
`bullpen news`). Every other line wakes, a new kind of line too.

To stop news reaching a coordinator unasked for a while, it runs
`bullpen news --pause` in its own pane (when you ask it to). The backstop then
types nothing to it; news is still logged, and `bullpen news` prints it.
`bullpen news --resume` ends the pause, and so does the next `bullpen up` for
that coordinator, with one line saying so. A pause has no timer. After a resume,
news logged during the pause gets a minute for a reader before the backstop
types it. With `type`, `--pause` refuses: there's nothing to pause.

You can switch while a coordinator runs. Switching to `monitor` types one last
line into its pane telling it to start the monitor (not when you run `config`
from the coordinator's own pane: it sees the answer). Switching to `type` makes
a running `news --follow` print `bullpen · news is typed into your pane now` and
exit, so the coordinator stops restarting it and no line arrives twice; in `type`
mode `--follow` always does that, and plain `bullpen news` still works. News
logged in `monitor` mode but not yet read when you switch back is not typed:
`bullpen news` prints it.

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
