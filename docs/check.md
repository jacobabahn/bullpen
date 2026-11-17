# No progress and status drift

## No progress

A worker can say it's working and be stuck: a command that hangs, a loop, a wait
on something Rex doesn't report as blocked. The watcher notes the time of every
status event from each pane. Once a minute it looks for panes that say they're
working and have sent none for 20 minutes, and reports each once: news
(``BP-3 has shown no progress for 20 min: `bullpen peek BP-3` ``), and on the
board the task gets the amber bar, a still amber mark and "No progress for 20
min." It stays in its group and nothing is typed into its pane: a long build
looks the same, so you (or the coordinator) decide. The pane's next status event clears it, and a new
quiet spell is reported again. The same goes for a reviewer
("BP-3's reviewer has shown no progress ...").

`bullpen config --stuck-after 45` changes the minutes for this repository; `0`
turns it off. The watcher only knows of events since it started, so a pane that
was already silent when the watcher (re)started isn't noticed until its next
event.

## Status drift

A task's status follows its agent's status events. One that goes missing (a
watcher or Rex restart, a bug) leaves the task out of step: marked working while
its agent sits at its prompt, say. `bullpen check` compares each open task that
has an agent in a pane (its worker, or its reviewer while in review) with what
that agent is doing now, and prints one line per mismatch:

```
BP-3 is marked working but its agent is idle at its prompt: `bullpen peek` it
BP-4 is marked needs (permission) but its agent is working again
BP-7 waits on its coordinator, but its coordinator didn't resolve it in 10 min: it needs the person
BP-5 is in review but its reviewer is idle without a verdict
BP-6 is marked working but its pane is gone
```

It prints nothing, and exits 0, when all agree (1 when something doesn't). It
reads the status record Rex keeps for each pane (the one the watcher's events come
from), and goes by the screen where Rex has none: Claude Code's spinner, its
prompt. An idle, blocked or error state that began in the last minute is left alone, since its event may
still be on the way, and so is a task bullpen itself moved in the last minute
(`start`, `tell`, `ready`): its agent has yet to report. `bullpen check BP-3` checks one task. An agent
idle after the coordinator's `bullpen ok` isn't drift, nor is a task waiting on an
`ask` whatever its agent does. `check` also passes a task left in triage too long to
the person (see [Triage](triage.md)), and finds a ready task that has no risk label
("BP-3 is ready without a risk label"), which `--fix` labels (see
[Review](review.md#the-risk-label)).

`bullpen check --fix` applies what the missed event would have done, news
included, as if it had come; it never types into an agent or approves anything.
The watcher runs it every 3 minutes, quietly, so drift fixes itself and you hear
of it once, in the usual words. The watcher's run (`--fix --all-repos`) also rebuilds
the watcher's own copy of bullpen when its source is newer than its build (see
[Rebuilds](rebuilds.md#the-watchers-check)).

An agent that ended its turn while a background shell of its own still runs
(Claude Code's footer says `1 shell` or `1 monitor`, or a worker's Stop hook said so: see
[Triage](triage.md#a-worker-that-ended-its-turn)) isn't finished: it takes another turn when
the shell ends. Bullpen counts it as busy, and holds back the "ended its turn
without `ready`" news for that turn. A shell it left running for good (a server, a
watch) would hide it, so once it has been idle that way for `--stuck-after` minutes
(20 by default), `check` reports it, and the news says a shell still runs. `check` also hints at an idle worker with
commits and a clean worktree that never ran `ready`; that one it leaves to you.

An agent that compacts its conversation by itself (Claude Code's auto-compaction:
`✽ Compacting conversation…` on its screen) says it's working, but that's no new
turn. Bullpen goes by the screen and leaves the task as it was: a ready or finished
task stays so, with its approval and summary, and a task in triage or waiting on you
stays there. When the compaction ends, its end of turn gives no news either, and
`check` sees no drift while it lasts. A real turn after it (typed input, `tell`, or
the agent going on by itself) counts as usual.
