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

To look at a task without leaving, `p` peeks at the end of its agent's screen, over
mission control. To work in its pane, `⏎` takes you there: the Rex app switches to
the task's repository's session, with its pane shown and focused; `cmd+;` `m` brings
you back. The pane stays where it lives (Rex can't show a pane in another session, so
it doesn't come to you), and its agent isn't touched.

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
 i answer  ⏎ open  t tell  p peek  n next  / find  q quit
```

- **The top line**: how many coordinators are blocked (`c●` and the count, on a narrow
  screen), what needs you (needs you or stopped), what's ready (ready to accept, or
  finished), how many of the open tasks have an agent on them (working or in review),
  and how many repositories; amber while a task needs you or a coordinator is blocked.
- **The inbox**: every repository's tasks that need you, then those that are ready,
  each with its repository, the one that has waited longest first: the order
  `bullpen go --next --all-repos` takes. Each has the board's marks, its age (red
  once it has needed you 5 minutes) and the amber bar, as on the board.
- **The repositories**: each one's line, with its folder, its coordinators and how many
  tasks are open; then its other open tasks (its coordinator on it, in review, working,
  queued) in the board's order. A coordinator is `●` working, `●` blocked (waiting on
  a permission prompt or a question in its pane: in bold amber, with the amber bar
  beside its repository's line, as a task that needs you has), `○` idle, `✕` gone (its
  pane closed: `bullpen up` there starts one), or `?` when the watcher's note on it
  can't be read. With no coordinator the line says so, in amber, as it does for one
  that's gone. A repository with task files that don't parse says how many after its
  count.
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

## Drawn as images

With `bullpen config --global --board-look image` (the board's [image
look](board.md#the-image-look); mission control reads the global setting, with
`board_text_size`, `board_font` and `board_motion`), mission control is drawn as the
board is: the same type, marks, cards, washes, chips and keycaps, in the terminal's
own colours. It says everything the text says, through the same keys.

- **The header**: "Mission control" in the pane's largest type, what's under way after
  it, dim ("3 of 13 running · 4 repos"), then at the right the chips: coordinators
  blocked and what needs you in amber, what's ready in green (their numbers alone when
  the words don't fit), and the time. Under it the fleet's bar: a run of segments for
  each repository, a segment a task in its group's colour (as the board's bar), and
  under each run its coordinator's mark, its name, and what needs you or is ready there
  when that fits. A blocked coordinator's repository is named in bold amber, a gone one's
  in amber. When the names don't fit on one line the runs wrap onto another, so a name
  is never cut (the words after it go first).
- **The list**: the board's rows. What needs you, then what's ready, under the
  board's group names, each task as its repository's board draws it, its
  repository's name before its title, dim, in a column of its own (cut past a fifth of
  the pane). Then each repository's heading: its name, its folder and how many are
  open, its coordinators at the right, each a mark and its state: working the working
  task's turning ring, blocked the amber disc of one that needs you (in bold amber),
  idle a dim ring, gone an amber cross. A blocked coordinator's heading sits on the
  amber wash with its bar, its question under it, as a task that needs you does.
  Without room, the folder goes first, then the coordinators' words (their marks stay).
  Under each heading, its other open tasks. The selected task sits on the board's card,
  with `⏎ open` in its corner (`⏎ reviewer` when ⏎ opens its reviewer's pane, as the
  board and the keys under the list call it).
- **The page**, beside the list from 80 columns: the selected task's ID, repository and
  folder, its title large, its mark and status in its colour and its age, its keys in
  caps (`p peek`, `⏎ open`, then its own), what its agent is doing, where it
  stands and the size of its change, and for a task in review, ready or finished what
  its worker said, under a rule. Under 80 columns there's no page: the list is the
  pane, and the card's lines are its detail, as on the board.
- **The bottom rows**: the message and the keys in caps, the selected task's then `n`
  `/` `q`, the task's dropping from their end to fit.

What's typed or asked at the bottom (a tell, a y/n question, `q`'s), a peek, a
prompt being answered and a coordinator's question stay text, as on the board; the
images come back after. With `board_look text`, on a terminal that doesn't draw
kitty graphics (or doesn't say its cell size), or when the font can't be read, it's
the text mission control, the message line saying why.

Each image is drawn at the pane's cell size as it is that frame, and drawn again when
it changes: a floating pane, first opened at a placeholder size, is drawn at its real
cell once it has one, never stretched. No image is over the terminal's limit (one that
would be is sent in strips), and everything kept stays under the pane's budget, as the
board's ([Images under the terminal's limit](code.md#images-under-the-terminals-limit)).
Moving the selection only places images: the list is the board's pieces, and while it
waits for a key mission control draws what `j` and `k` would show next (the selected
block and the page) and sends it unplaced. A key is about a millisecond and under a
kilobyte, as the board's.

`bullpen-board --mission --state <state folder> --bullpen <program> --render FILE`
writes mission control as the image look draws it to FILE (`--cols N`, 160 by
default; `--theme light` or `dark`), without a terminal.

## Keys

| Key | What it does |
| --- | --- |
| `j` `k` (`↑` `↓`) | select a task; `PgUp` `PgDn`, `ctrl+u` `ctrl+d`, `Home` `End` (`G`) as on the board |
| `n` | the next task that needs you, in any repository (again: the one after); once nothing does, the next that's ready |
| `p` | peek at its worker's screen (its reviewer's, in review), over mission control and updating, as the board's `p` does; `esc` or `q` returns, and `t` `⏎` `i` `a` `d` `v` act on that task from it |
| `⏎` | go to its pane (its reviewer's, in review): the Rex app shows that repository's session, focused on the pane (`bullpen go --switch`); a task with no pane says so and does nothing |
| `i` | answer it: its agent's prompt, or its coordinator's question |
| `a` then `y` | accept it, or close a finished one |
| `d` then `y` | drop it |
| `t` | type a line to its worker (`bullpen tell`) |
| `s`, `r` | start a queued task, resume a stopped one |
| `g`, `v` | its tour, its diff, in a floating pane over mission control |
| `/` | the picker, over mission control: any open task, in any repository |
| `q` then `y` | quit (`cmd+;` `m` starts it again) |

The last line is the selected task's keys, as on the board, then mission control's
own (`n` `/` `q`); keys drop from the task's end when the screen is narrow.

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

The watcher notes Rex's status of each coordinator's pane (working, blocked or idle)
in `doing/`, as it does a worker's, a coordinator made with `bullpen up --here` too,
so a coordinator's state is as current as a task's.

Still to come: a panel for the coordinators,
news and health panels, zoom, and a repository's whole board as a drill-in.
