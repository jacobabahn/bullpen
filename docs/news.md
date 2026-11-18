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
- `wake`: nothing is typed, and the coordinator starts nothing. Bullpen writes a
  hook into the repo's `.claude/settings.local.json`; each time the coordinator's
  turn ends, the hook parks a waiter, which costs no turn while nothing happens and
  wakes the idle session when news comes (see [The waiter](#the-waiter-config---news-wake)
  below). It needs Claude Code as the coordinator. It is opt-in for now.
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
start it again. A session with `wake` has no waiter until its first turn ends (a
resumed one, say), or when the hook can't run. So news that isn't typed has a backstop. When a line worth
acting on is logged, the watcher looks every 3 seconds (`bullpen _wake_check`,
while `<state>/wake-due` exists). Once such a line has gone a minute unread by
a coordinator whose pane is running, and no reader takes its news (a
`news --follow` holds `news-c1.lock`; a waiter, below, is named in
`news-c1.wait` while its process runs), bullpen types every line since the coordinator's cursor
into its pane, as `type` would, and moves the cursor: each line is typed once,
and `bullpen news` won't print it again. It waits behind a draft in the input,
as typed news does; if a draft holds it for 10 minutes, you get one
notification for that batch ("Your coordinator (c1) has news it hasn't seen").

A line that asks the coordinator to do something wakes it; one that needs nothing
rides along with the next line that does (or waits for `bullpen news`). Each kind of
line is in one table (`cli/src/news/kind.rs`), with the row of the guide's news
table it belongs to, and that row decides. What rides along: `accepted`, `closed`,
`kept`, `dropped`, `started` and `sent back` lines; an `After BP-3: ...` line where
every task was brought up to date (event `rebased`); `not auto-accepted (nothing to
do ...)`; pickup's "started its 4 for today, nothing to do" (`pickup`); the queue's
"can start, but the usage limit is reached" and revert's "isn't started" (`queue`);
and a worker's backlog item (`later`). Everything else wakes (bullpen writes no
line without a kind; an event it doesn't know, in an old log, wakes too): among them the keys-changed line (`keys`), pickup's
asks (`pickup-ask`), the queue's "couldn't start" (`queue-failed`), an `After`
line with a task not rebased, whose push failed or that a message didn't reach
(`not-rebased`), and auto-accept's "approved by review and auto-accepted" and
"closed by auto-accept" lines, which the coordinator reports (`auto-accepted`,
`auto-closed`). Two kinds still wake though their row needs nothing: `working
again` lines, and auto-accept's "sent back to its worker to rebase". The line for
new [GitHub issues](issues.md#on-a-timer) wakes: each of them asks you.

## The waiter (`config --news wake`)

`bullpen config --news wake` writes a Stop and a StopFailure hook into the repo's
`.claude/settings.local.json`, the settings Claude Code reads for a session in that
folder and doesn't share:

```json
{"hooks": {
  "Stop": [{"hooks": [{"type": "command", "command": "/path/to/bullpen news --wake",
                       "asyncRewake": true, "timeout": 86400}]}],
  "StopFailure": [{"hooks": [{"type": "command", "command": "/path/to/bullpen news --wake",
                              "asyncRewake": true, "timeout": 86400}]}]
}}
```

- The file is yours and Claude Code's. Bullpen knows its own entries by their
  command's `news --wake` ending and touches nothing else: your other settings and
  hooks stay, in their order. It writes the file by rename.
- `bullpen config --news type` (or `monitor`) takes the entries out again, with the
  lists that leaves empty, and the file if nothing else is in it. A parked waiter
  ends within a second.
- The file is added to the checkout's `.git/info/exclude`, so git leaves it out: it
  holds your own path to bullpen.
- `wake` is refused when the coordinator isn't Claude Code (`coordinator_cmd`'s
  program isn't `claude`): only it has the hook. Use `type` there. It is also
  refused, with nothing changed, when the settings file doesn't parse.
- `bullpen up` checks that the entries are there and run the copy of bullpen the
  repo's coordinator uses, and writes them again if not ("News hook written to
  .claude/settings.local.json"). `bullpen doctor` checks the rest (see
  [Doctor](doctor.md)).
- Claude Code reads the file again when it changes, so the switch reaches a
  coordinator that is running. Every other Claude session in the folder runs the
  hook too; there it exits at once.

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
  pane, unless news is `wake`, or while news is paused, it exits at once: that is
  all another Claude session in the folder pays for the hook.
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
- A worker waiting on its own background job isn't news at all until the wait
  outlasts `stuck_after`: workers have a Stop hook of their own, in their
  worktrees, which says what they wait on (see [Triage](triage.md#a-worker-that-ended-its-turn)).
  It is a different hook from this one, and needs nothing from you.
- It ends by itself a minute before the hook's timeout, since a hook killed at
  its timeout loses its exit 2. News that waits then is handed over at once; with
  none, the session has no waiter until its next turn ends, and the backstop
  covers it. For a hook with another timeout, say so: `news --wake --timeout
  3600`.
- When the session exits or the pane closes (TERM, HUP, INT) it exits without
  moving the cursor, so the next waiter, `bullpen news` or the backstop has the
  lines.
- A session that is killed outright (`kill -9`, a crash) signals nobody. The
  waiter notices within a second that nobody reads its stderr any more and
  exits the same way; and it moves the cursor only once its lines got out.
- It shares the cursor and the lock with `bullpen news`, so no line comes twice.
  The backstop types nothing while a waiter lives. With `wake`, `news --follow`
  prints `bullpen · news wakes you by itself now` and exits.
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

You can switch while a coordinator runs. Switching to `wake` types one last line
into its pane saying that news wakes it by itself from now on; the turn that line
starts ends with the hook in place, so it parks the first waiter. Switching to
`monitor` types one last line telling it to start the monitor. (Neither is typed
when you run `config` from the coordinator's own pane: it sees the answer.)
Switching from `type` starts from the end of the log, since what was typed isn't
news again; between `monitor` and `wake`, unread news stays unread. Switching to `type` makes
a running `news --follow` print `bullpen · news is typed into your pane now` and
exit, so the coordinator stops restarting it and no line arrives twice; in `type`
mode `--follow` always does that, and plain `bullpen news` still works. News
logged in `monitor` or `wake` mode but not yet read when you switch back is not typed:
`bullpen news` prints it.

## The guide, and what a news line has to say

The coordinator's guide is `prompts/coordinator.md`: the rules, how to plan and brief,
the commands, how to read news, triage, and how to report to you. Sections that only
some repositories need are files of their own in `prompts/coordinator/`, and a
repository's guide has one only when it applies:

| Section | File | In the guide when |
|---------|------|-------------------|
| News by wake | `news-wake.md` | `news` is `wake` |
| News by monitor | `news-monitor.md` | `news` is `monitor` |
| Pull requests | `prs.md` | the repository has an `origin` remote |
| Auto-accept | `auto-accept.md` | `auto_accept` is on, or `pickup` with `pickup_auto_accept` |
| Pickup | `pickup.md` | `pickup` is on |
| GitHub issues | `issues.md` | `issues` is on |
| Other coordinators | `others.md` | the repository has more than one coordinator |

`bullpen guide` prints the guide for the repository it runs in; `bullpen guide --all`
(and `bullpen guide` outside a repository) prints it with every section.

The guide doesn't list news lines. It has one rule and a short table: a line says
what happened and, when there is something for the coordinator to do, what (a command
to run, or who to tell); a line that says nothing of the kind matches a row of the
table (`For the coordinator:`, a task that needs you, your answer, ready or finished,
the lines that need nothing, two tasks that changed the same file). So a new news line
has to carry its own instruction or fit a row. The tests hold every line they send to
that: `tests/support.py` (`check_news`) fails a test whose news does neither, and
`tests/test_guide.py` checks that the table's rows are the ones the tests use.

## A changed guide

When the coordinator's guide changes. A coordinator gets its guide once: in its
system prompt, or by reading `bullpen guide`
after `up --here`, and a running agent can't load it again. So each coordinator
records the hash of the guide it was given, and the text is kept in
`~/.local/state/bullpen/guides/<hash>.md`. When `bullpen up`, `bullpen accept` (a
pull or a merge may have changed it) or `bullpen config` (a setting may have brought
a section) finds that a running coordinator's guide is
out of date, in any repository, it sends that coordinator alone one news line, and
records the new hash, so each change is told once.

When a setting brought the guide a section (auto-accept, pickup or issues turned on, a second
coordinator started, a remote added), the section comes after the line, whole:

```
bullpen · Your coordinator guide changed with this repo's settings: it has the section Auto-accept now. Read it below and follow it from now on:
## Auto-accept
...
```

A section that only stopped applying is no news: following it does no harm. Only
these six sections are told this way: a section a merge adds to the guide or takes out
of it is a change like any other, told by diff. A switch
of `news` to `wake` or `monitor` needs no such line either: the last typed line, which
says news is no longer typed, carries the section for the new way.

For any other change, when the diff is short (40
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
bullpen · Your coordinator guide changed (sections: Plan, News), 63 lines of diff: run `bullpen guide` to read it all, and follow it from now on.
```

A coordinator from before this (no hash),
or one whose old text is gone, is told once that its guide may have changed, to run
`bullpen guide`, with no diff.
