# Stats: where each task's time went

Each task records a timeline of what happened to it. `bullpen stats` adds it up, so
you can see what costs the most per task and judge a change to how you work by
measuring it.

```sh
bullpen stats BP-3              # BP-3's timeline and totals
bullpen stats                   # the totals across closed tasks
bullpen stats --since 2026-11-01   # closed since then (also a stamp, or days back: 7d)
bullpen stats --last 20         # the last 20 closed
bullpen stats BP-3 --json       # as JSON, for a coordinator or a script (both forms)
```

## One task

`bullpen stats BP-3` prints the task's timeline, one line per event, and then:

- **Time** in each phase:
  - working
  - rebasing: working after it was sent back to rebase, until it's ready again
  - waiting for review: in review before its reviewer started (reviews start at `ready`, so this is usually none)
  - in review
  - waiting for the person: a question or prompt (needs you), or ready or finished and not accepted yet
  - waiting for the coordinator: its worker ended its turn without `ready`
  - stalled: a stop the coordinator sees to (its turn failed, its pane closed or was lost, a message that didn't reach it, the usage limit), or stopped
  - queued: added and not started yet
- **Review rounds**, and the reviews by kind (full, light, followup, rebase check,
  rebased, tour).
- **Trips back to work**, by reason:
  - review: a reviewer asked for changes
  - rebase: it conflicted with its base
  - person: told more after `ready`, or tour comments
  - pr: comments on its PR
  - other: its worker took a new turn of its own after `ready`
- **Stalls**, **questions** and **turns** (each time its worker ended a turn).
- **Auto-accept refusals**, by reason (conflict, checks, comments, other).
- **Changes**: its commits and lines added and removed, as its last `ready` counted them.

An open task's time counts up to now.

## Across tasks

`bullpen stats` with no ID covers closed tasks (accepted, closed or dropped). It
shows how many ended each way. Then, for each phase, the median per task, the total,
its share of all the time, and the three slowest tasks. Last come the time from added
to closed, the review rounds, how often each kind of trip back happened (and in how
many tasks), stalls, questions, auto-accept refusals, and the commits and lines.

## Tasks from before timelines

Tasks that closed before timelines were recorded have none of their own. Their
timeline shows their lines from the news log instead. Those lines don't say every
change of status, so their phases, trips back and stalls show as unknown (null in
JSON). What the task itself records is still given: when it was added and closed,
its review rounds, commits and lines. A task that started before timelines were
recorded has a partial timeline, from the first event recorded. Its earlier time is
unknown. The totals across tasks leave both kinds out of the phases and name them.

## What is recorded

Each task's timeline is one line of JSON per event, in
`timeline/<ID>.jsonl` beside the tasks (see [Where things live](where-things-live.md)).
Every line has `at` (a stamp) and `event`:

| event | when | with |
| --- | --- | --- |
| `added` | the task was added | `status` |
| `status` | its status changed | `from`, `to`; `stop` and `why` for a stop; `ask` for a question to the person; `back` for a trip back to work (`review`, `rebase`, `person`, `pr`, `limit`); `outcome` for a closed one (`accepted`, `closed`, `kept`) |
| `turn_ended` | its worker ended a turn | |
| `review_started` | a reviewer started | `round`, `kind` |
| `review_finished` | it gave its verdict | `round`, `kind`, `verdict` |
| `accept_refused` | an auto-accept didn't merge it | `reason` (`conflict`, `checks`, `comments`: new on its PR while it waited for checks, `other`), `why` |

Saving a task writes it: the save compares the task with what it was, so commands
that only read tasks (`ls`, the board) write nothing. A trip back gets its reason
from the step that sends it back. An auto-accept refusal changes nothing a save would
see, so the auto-accept records it itself.

Tokens and cost aren't recorded. Claude Code keeps each session's usage in its own
files, but nothing ties a session there to a task's worker or reviewer without
guessing.
