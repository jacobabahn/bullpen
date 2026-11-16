# The backlog

Ideas that aren't tasks yet go on the backlog: yours, the coordinator's, and a worker's
aside about something outside its task. It is one list per repository, in order (first
is next), its items named `L1`, `L2`, ... so they never look like a task (`BP-3`).

An item becomes a task when someone starts it: you, or the coordinator. Nothing starts
by itself yet.

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
(`backlog`) and `started_by` (`person`, or the coordinator's ID). `bullpen show` says it:

```
     from: the backlog (L7), started by c1
```

The item becomes `started`, its `task` the new task's ID. A task dropped or closed with its
branch kept leaves its item `started`: it was tried. `bullpen later "..."` again to retry.

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
`dropped` or `done`. `note` is why it was dropped; `skipped`, why it was passed over.

The code: `core/src/backlog.rs` (the item, loading and saving under the lock, the order),
shared with the board, and the command in `cli/src/backlog/`.
