# The backlog

Ideas that aren't tasks yet go on the backlog: yours, the coordinator's, and a worker's
aside about something outside its task. It is one list per repository, in order (first
is next), its items named `L1`, `L2`, ... so they never look like a task (`BP-3`).

An item becomes a task when someone starts it: you, or the coordinator. With pickup on
(off by default), bullpen also starts the next one by itself when there's room: see
[Pickup](#pickup).

## Commands

```sh
bullpen later "Tidy the watcher's error messages"   # add it at the bottom; prints L7
bullpen later "Fix the flaky stuck test" --top      # add it at the top
bullpen later "..." --brief-file b.md               # with a brief: what a worker would do
bullpen later                                       # the list, in order
bullpen later --json                                # the same, as JSON (--all: every item)
bullpen later L7                                    # one item in full, and what can be done with it
bullpen later L7 --top                              # move it: --top, --up, --down, --bottom
bullpen later L7 --brief-file b.md                  # give it a brief
bullpen later L7 --keep                             # yes: an item that asks goes on the list
bullpen later L7 --drop "not worth it"              # no (the why is optional)
bullpen later L7 --skip "overlaps BP-138"           # pass over it this time, and why
bullpen later L7 --start [--brief-file b.md]        # make it a task now and start it
```

The list says each item with a mark: `◇` on the list, `?` asks for a yes. Items that
became tasks follow, dim, with their task and its status (`↳ L5  Fix the flaky test ·
BP-140, working`), until that task closes. `--all` adds the dropped ones and the started
ones whose task closed.

A flag about one item with no item named (`later --top`) is refused, not taken for the
list. A worker may view an item (`later L7`) but not change it.

`--start` refuses an item with no brief: a worker needs one. Give it one first, or with
`--brief-file` on the same command. It runs `add` with the item's title and brief, then
`start`; if the start fails (no Rex session, the usage limit), the task is made anyway and
the item records it: `bullpen start BP-140` once it can.

## Who adds what

- **You** (a shell, or a pane that isn't bullpen's) and **the coordinator**: the item is on
  the list at once. Its `by` says who: `person`, or the coordinator's ID (`c1`).
- **A worker**: its footer says to `bullpen later` what it notices outside its task, not to
  do it. bullpen knows the worker from its pane (or from `--from BP-131`), and the item
  *asks*: it waits for a yes. The worker's coordinator hears it:

  ```
  bullpen · BP-131 added L8 to the backlog: Split watch.lua's timers (`bullpen later L8 --keep` puts it on the list, `--drop` says no)
  ```

  The coordinator says yes to one that's clearly small and worth it; the rest wait for you.
  A worker can only add: keep, drop, move and start are the coordinator's and yours.

## A task from the backlog

A started item's task carries where it came from: `from` (the item, `L7`), `origin`
(`backlog`) and `started_by` (`person`, the coordinator's ID, or `itself` for pickup). `bullpen show` says it:

```
     from: the backlog (L7), started by c1
```

The item becomes `started`, its `task` the new task's ID. A task dropped or closed with its
branch kept leaves its item `started`: it was tried. `bullpen later "..."` again to retry.

## Pickup

`bullpen config --pickup on` lets bullpen start the backlog's next item by itself while
there's room. It is off by default, and per repository or `--global` like any setting.
Turn it on yourself: nothing else does.

When it runs:

- after a save that frees a slot (a task accepted, closed, dropped, gone to review, ready
  or finished), right after the run queue, so tasks waiting with `--after` go first;
- after `later` changes the list (an item added, given a brief, kept, skipped, moved),
  and after `bullpen up`;
- from the watcher, every 5 minutes, while `~/.local/state/bullpen/pickup` exists
  (some repository has pickup on and a kept item): it catches what no save does, like
  the usage limit's reset or pickup just turned on. With nothing to do it costs one
  file check.

It starts at most one item a run, and only when all of these hold:

- **Nothing needs you**: no task in the repository needs you or is stopped. A question
  you haven't answered comes before new work.
- **No pile**: fewer than `pickup_waiting` (2) self-started tasks wait for you (ready,
  finished, needs, stopped). At the cap it pauses, with one news line and one
  notification, once a pause: `Pickup paused: BP-140 and BP-141 wait for you`.
- **Room**: fewer than `max_running` tasks work, and fewer than `pickup_running` (1)
  self-started ones work or are in review.
- **Budget**: fewer than `pickup_per_day` (4) self-started tasks today (local day, closed
  ones too). Once spent, one news line says so for the day.
- **Not at the usage limit**, and a Rex session to start the worker in.

Then it takes the first kept item (an item that asks waits for its yes), passing over
skipped ones:

- **With a brief**, it starts it: one news line, `BP-140 started by itself from the
  backlog (L7: Fix the flaky stuck test)`. The task's `started_by` is `itself` (`show`:
  "started by itself"), it is kept for you to accept even with `auto_accept` on (unless
  `pickup_auto_accept`), and its worker gets `pickup_model` if set. A skip lasts until
  this start: every item's `skipped` is cleared then.
- **Without one**, it asks the coordinators for one, once while that room lasts:

  ```
  bullpen · Room for one from the backlog: L7 Tidy the watcher's errors. Write its brief and give it with `bullpen later L7 --brief-file <f>` (pickup then starts it), or `bullpen later L7 --skip "why"`
  ```

  Giving the brief runs pickup, which starts it. With a coordinator running, pickup waits
  for it: the order is yours. With none, the line waits in the news for the next one, and
  pickup tries the next item that has a brief.

A hand start (`later L7 --start`, by you or the coordinator) isn't pickup's: it is not
capped and keeps the config's auto-accept, as `add` and `start` would.

`bullpen later` ends with pickup's line while it is on: `Pickup on (1 of 4 started
today)`, `Pickup waits for room`, `Pickup waits: a task needs you`, `Pickup paused: BP-140
and BP-141 wait for you`, `Pickup done for today (4 of 4 started)`, or `Pickup waits: the
usage limit is reached`.

The settings, in `bullpen config`'s Pickup group:

| Setting | Default | What it does |
| --- | --- | --- |
| `pickup` | off | start backlog items by themselves when there's room |
| `pickup_running` | 1 | self-started tasks working or in review at once |
| `pickup_waiting` | 2 | pickup pauses while this many self-started tasks wait for you |
| `pickup_per_day` | 4 | self-started tasks a day, a spending guard |
| `pickup_model` | the worker model | claude's `--model` for self-started tasks |
| `pickup_auto_accept` | off | self-started tasks accept by themselves once review approves them |

`pickup.json` beside the tasks keeps what a room has asked for and whether pickup is
paused, so each is said once; `pickup.lock` keeps two pickups from both taking one room.
The code is `cli/src/backlog/pickup.rs` (`_pickup` is the watcher's call).

## Where it lives

`backlog.json` in the repository's state folder (beside `tasks/`), read and written under
its lock (`backlog.lock`) so two commands adding at once both land; an item's brief is
`briefs/<ID>.md` there. An item:

```json
{
  "id": "L7",
  "title": "Tidy the watcher's error messages",
  "added": "2026-11-13T11:40:02-08:00",
  "by": "person",
  "state": "kept",
  "brief": "L7.md",
  "note": null,
  "task": null
}
```

`state` is `asks` (waits for a yes), `kept` (on the list), `started` (became a task),
`dropped` or `done`. `note` is why it was dropped; `skipped`, why it was passed over
(until pickup's next start).

The code: `core/src/backlog.rs` (the item, loading and saving under the lock, the order),
shared with the board, and the command in `cli/src/backlog/`.
