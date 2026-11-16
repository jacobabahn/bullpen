# Mission control

Mission control is every repository bullpen knows on one screen: what needs you in
any of them, what's ready, and what each one's agents are doing. Each repository
keeps its own coordinator, board and tasks; mission control sits above them and acts
through the same `bullpen` commands the board runs.

`cmd+;` then `m` (or `bullpen mission` from any pane or shell) goes there. It lives
in a Rex session of its own, `mission`, with one pane. The first time, it makes that
session; after that it focuses the one that's running. If its pane was closed (or
Rex restarted), it opens a new tab in the `mission` session rather than a second
session. `bullpen mission --here` runs the screen in the terminal you're in, which is
what its pane runs. Where it runs is kept in `mission.json` in the state folder.

To get back to a repository from it, `⏎` goes to the selected task's pane; `cmd+;`
`m` brings you back.

## What it shows

```
 MISSION CONTROL   2 need you · 1 ready · 3 of 7 running · 2 repos                     Sun 15 Nov 19:54
┌ Inbox and repos ─────────────────────────────────┐┌ Detail ────────────────────────────────────────┐
│ NEEDS YOU                                        ││ BP-12  Split the rebase planner                │
│▌● jj-evolved BP-12   Split the rebase planner 14m││ jj-evolved · ~/dev/jj-evolved                  │
│▌     Asks: Allow `jj git push --all`?            ││                                                │
│▌■ bullpen    BP-126  Measure the test suite…  3m ││ ● needs you · 14m                              │
│▌     Its agent exited. r resumes it.             ││ Asks: Allow `jj git push --all`?               │
│ READY                                            ││                                                │
│ ✔ bullpen    BP-127  bullpen stats           22m ││                                                │
│      PR #141, approved by full review.           ││                                                │
│                                                  ││                                                │
│ bullpen  ~/dev/bullpen   c3 ● working     5 open ││                                                │
│ ◜ BP-129  close --keep-branch docs            4m ││                                                │
│ ⠋ BP-130  Design: mission control            31m ││                                                │
│ ◌ BP-132  Board split, step 1                now ││                                                │
│                                                  ││                                                │
│ jj-evolved   c1 ○ idle                    2 open ││                                                │
│ ⠋ BP-13   Evolve across merges               48m ││                                                │
└──────────────────────────────────────────────────┘└────────────────────────────────────────────────┘
 i answer  ⏎ open  t tell  n next  / find  q quit
```

- **The top line**: what needs you (needs you or stopped), what's ready (ready to
  accept, or finished), how many of the open tasks have an agent on them (working or
  in review), and how many repositories; amber while something needs you.
- **The inbox**: every repository's tasks that need you, then those that are ready,
  each with its repository, the one that has waited longest first: the order
  `bullpen go --next --all-repos` takes. Each has the board's marks, its age (red
  once it has needed you 5 minutes) and the amber bar, as on the board.
- **The repositories**: each one's line, with its folder, its coordinators and how many
  tasks are open; then its other open tasks (its coordinator on it, in review, working,
  queued) in the board's order. A coordinator is `●` working (or blocked on a question
  in its pane), `○` idle, `✕` gone (its pane closed: `bullpen up` there starts one), or
  `?` when the watcher's note on it can't be read. With no coordinator the line says
  so, in amber, as it does for one that's gone. A repository with task files that
  don't parse says how many after its count.
- **Under each task** (in the inbox, and every task on a wide screen): what its agent
  is doing now, else where it stands, in the board's words. When the watcher's note on
  an agent can't be read, the task says where it stands, as with no note.
- **The detail**, beside the list: the selected task's title, its repository, its
  status and age, what its agent is doing, where it stands (with auto-accept and the
  model it chose), the size of its change and its tour, and for a task in review,
  ready or finished, what its worker said it did.

It lays itself out for the terminal it's in. From 140 columns, the list and the detail
sit side by side and every task gets a line under it. From 80 columns, the same, but
the repositories' tasks are a line each. Under 80 there is one column, the
coordinators are marks, and the selected task's detail is under it in the list.

## Keys

| Key | What it does |
| --- | --- |
| `j` `k` (`↑` `↓`) | select a task; `PgUp` `PgDn`, `ctrl+u` `ctrl+d`, `Home` `End` (`G`) as on the board |
| `n` | the next task that needs you, in any repository (again: the one after); once nothing does, the next that's ready |
| `⏎` | go to its pane (its reviewer's, in review), as `bullpen go` does |
| `i` | answer it: its agent's prompt, or its coordinator's question |
| `a` then `y` | accept it, or close a finished one |
| `d` then `y` | drop it |
| `t` | type a line to its worker (`bullpen tell`) |
| `s`, `r` | start a queued task, resume a stopped one |
| `g`, `v` | its tour, its diff, in a floating pane over mission control |
| `/` | the picker, over mission control: any open task, in any repository |
| `q` then `y` | quit (`cmd+;` `m` starts it again) |

Each key on a task runs the `bullpen` command in that task's repository, as that
repository's board does, with the same questions (`y / n` on its row, red when risky),
the same answer screen and the same line to type on. Accept, drop, start and resume run
in the background; their spinner turns on the task's mark meanwhile.

## How it stays current

It reads the same snapshot as `bullpen repos` (core's `fleet.rs`): every repository's
settings, coordinators, tasks and the watcher's notes. It watches the whole state
folder and reads the snapshot again when anything there changes, and once a second in
case a change was missed; a file is read again only when its time or size changed.
Whether each coordinator's pane is alive is Rex's to say: `bullpen repos --json` asks
it every 5 seconds, in the background. When its program is rebuilt it reloads itself,
keeping the selection, as the board does.

Still to come: a live look at the selected task's pane, a panel for the coordinators,
news and health panels, zoom, and a repository's whole board as a drill-in.
