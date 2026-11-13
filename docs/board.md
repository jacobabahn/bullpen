# The board

The task board (`bullpen board`, a Rust program in `board/`) sits in a narrow
pane beside the coordinator and updates as soon as a task changes.
It groups tasks by what you do next: needs you, coordinator on it (dimmed: see
[Triage](triage.md)), ready to accept, finished (nothing to merge, see
[Finish](finish.md)), in review, working, queued; `f` adds
the closed ones (accepted, closed, dropped). `l` switches to a compact layout,
one line per task under its group's name (dim), with the selected task's detail
at the bottom (what its agent is doing first). A board with tasks but none to
show says why: "Nothing open. f shows 2 closed.", or "Nothing of yours open. o
shows 4 of c3's." Under 12 rows the list takes the blank rows, the scroll mark
sits at the end of its last row, and a message takes the key row; under 36
columns an ID is followed by one space, and (grouped) a task's age starts its
detail line.

Each task starts with its mark, the same one `?` shows under "Marks":

| Mark | The task |
| --- | --- |
| `●` amber | needs you |
| `■` red | stopped: its agent exited |
| `◎` dim | the coordinator is on it |
| `✔` green | ready to accept |
| `◆` green | finished, nothing to merge |
| `◜` blue, turning slowly (`◐` still) | in review |
| `⠋` turning slowly (`○` still) | working |
| `◌` dim | queued, not started |
| `·` dim | accepted or closed |
| `–` dim | dropped |
| `⣾` turning, in place of the mark | a command (accept, drop, start, resume) or its tour being written |
| `▌` at the left edge | needs you (amber), stopped (red), or no progress (amber) |

A turning mark stands still, in amber, when its agent has shown no progress for
a while (see [No progress](check.md#no-progress)), and in its own colour at the usage
limit. The working and review marks turn five times a second, slower than a command's
spinner, so they stay calmer than its feedback. Each task shows how long
it has been there; a task that needs you or stopped turns its age red after 5
minutes. On the lines under it (from under its ID) it shows
what it needs or where it stands ("Asks: python3 or python?", "PR #4, approved
by light review.", "Follow-up review on round 2 of 2."): one or two lines, cut
to fit, and for the selected task all of it, up to eight lines. The lines of a task
that needs you, stopped, is ready or finished are at normal weight; the rest
(working, queued, in review, the coordinator's, closed, and a change's size) are
dimmed. The selected
task has its ID in reverse video, which shows in any theme, and a tint under its
whole block, title and lines. A working task shows there, dimmed, what
its worker is doing now, and a task in review what its reviewer is doing: the
text of the agent's `working: ...` status, cut to fit and updated as it changes.
With no such text, or text from before the task's current status or over 30
minutes old, it shows the usual line. A task in review or ready also shows the
size of its change ("3 files, +120 −14"), worked out once when its worker runs
`ready`.

The top line counts only what you act on, at its right: `2 need you · 3 ready`
(need you: needs or stopped; ready: ready to accept, or finished), in amber and
green, shortened to `●2 ✔3` when the words don't fit. Before it come the other
repositories with tasks that need you or are ready (see [Several
repositories](several-repositories.md)), which get the room first. On a crowded
line the hidden tasks' note drops its "o shows" first, then this repository's
counts shrink to marks, then to what needs you alone; the other repositories go
only when even their marks don't fit.

`bullpen ls`, `bullpen show` and the picker (`cmd+;` `j`) say a task the way the
board does: its mark (from the table above), its ID, title and age, then the
board's line for it, needs-you first as on the board.
Piped, `ls` prints each task whole on one line; on a terminal it is in colour and
each line is cut to the width. When that would leave too little room for the line
(a narrow terminal like the board's), it takes the board's two rows: title and
age, then the line under it. `--json` keeps the raw status.
`show` starts with that line and ends with `next:`, what you can do now
(`next: bullpen reply BP-3 <answer>, or i on the board`).

While workers or reviewers float, the board leaves its bottom row empty: Rex draws
minimized panes (their capsules) over the bottom right of the window, which would
cover the end of the key line. With neither floating there are no capsules, and no
gap. `bullpen config --board-bottom-gap 0` takes the row back either way (or give
more rows); a board too short to spare it drops the gap first.

The key line at the bottom shows the selected task's keys. The board's keys,
in the order `?` lists them:

| Key | Does |
| --- | --- |
| `j` `k` (`↑` `↓`) | select a task |
| `PgDn` (`ctrl+d`: half as far) | select the task a page down: as many tasks as the board shows; the list scrolls to keep it in view |
| `PgUp` (`ctrl+u`: half as far) | select the task a page up |
| `G` (`End`) | select the last task; `Home` the first (`g` is for tours) |
| `n` | select the next task that needs you; once nothing does, the next one ready to accept, then finished ("Nothing needs you: BP-3 is ready.") |
| `⏎` | open its worker's pane, or its reviewer's |
| `c` | back to the coordinator |
| `/` | the picker, as `cmd+;` `j` opens it: type part of any open task, in any repository, and go to it |
| `a` | accept a ready task (merge it), or close a finished one; asks y/n first |
| `d` | drop it; asks y/n first |
| `i` | answer its question, on a task that needs you (see [Answering from the board](#answering-from-the-board)) |
| `t` | tell its worker something |
| `p` | peek at its worker's screen, in its colours, updating until `esc` or `q`; `t` `⏎` `i` `a` `d` `v` in a peek go back to the board and act on that task |
| `v` | view its diff |
| `g` | its guided tour (see [Guided review tours](tours.md), with the keys inside a tour) |
| `s` | start it |
| `r` | resume a stopped worker |
| `l` | switch layout: compact, one line a task, or back |
| `f` | show the closed tasks too |
| `o` | show other coordinators' tasks too (see [Several coordinators](several-coordinators.md)) |
| `esc` | put a red message away (see below) |
| `?` | every key, one short line each, and the marks |
| `q` | quit the board, once you say `y` (`ctrl+c` quits at once) |

The `y`/`n` question for `a` and `d` goes above the key line, and the task keeps
its title with `y / n` in place of its age. It says which of the two it is, and in red,
first, what could go wrong: checks failing or still running, changes asked for,
not reviewed, a worker or reviewer still running; a drop is always red. Once
an accept or close goes through, the next task that needs you is selected, else
the next one ready or finished, so you can run down a queue of ready tasks with
`a`, `y`. On a narrow or short
board, `j` `k` `space` `b` scroll the `?` list and any other key closes it. The
key line always ends in `?`: when the task's keys fill it, its last keys give
way, not `?`. `bullpen up --no-board` leaves the board out. Accept, drop, start
and resume run in the background, with a spinner on the task, so the board stays
usable meanwhile.

What a key did shows above the key line, wrapped over up to three rows: green
when it worked ("Accepted BP-7."), red when a command failed, with its reason,
and plain otherwise (`l` says "Compact layout.", `o` "Showing c2's tasks too.",
`f` "Showing closed tasks too."). A peek that can't read the screen says why
(its agent isn't running), and its foot drops "live".
A green or plain message goes with the next key; a red one stays while you move
around, until the next thing you do to a task, or `esc`.

Accepting or dropping from the board (or a shell) is news to the coordinator:
`BP-3 accepted (merged PR #12, rebase)`, `BP-3 dropped`. Run from the
coordinator's own pane, `accept` and `drop` post nothing: it already knows.
Tasks that waited on the accepted one start by themselves (see
[Dependencies](dependencies.md)).

## Typing a line

A tell (`t`), an answer (`i`), the picker's search, and a comment or tell in a
tour all take the same editing keys:

| Key | Does |
| --- | --- |
| `←` `→` (`ctrl+b` `ctrl+f`) | move a character |
| `alt+←` `alt+→` (`alt+b` `alt+f`, `ctrl+←` `ctrl+→`) | move a word |
| `Home` `End` (`ctrl+a` `ctrl+e`) | go to the start or the end of the line |
| `backspace` `delete` | delete a character before or at the cursor |
| `ctrl+w` (`alt+backspace`) | delete the word before the cursor; `alt+d` the one after |
| `ctrl+u` `ctrl+k` | delete to the start or the end of the line |

There is no length limit. A long line shows the part around the cursor. Pasted
text goes in whole: a line break in it is a space, so a paste never sends the
line, and a paste on the board with no line open does nothing. A tour comment
can have more than one line: `ctrl+j` (or `alt+⏎`) starts a new one, `↑` `↓`
move between them, and a paste keeps its line breaks. In the picker, `ctrl+k` and
`↑` `↓` still move the selection.

## Answering from the board

A task that needs you for a question gets `i answer` on its key line:

- **At a prompt** (its worker or reviewer asks permission, or to choose): `i` shows
  the end of its screen, as `p` does, with the prompt's options under it, each with
  its key. Press one (`1`, `2`, ..., or `y`, `n` for a "(y/n)" question), and bullpen
  presses it in the agent's pane. It reads the screen again first, and sends nothing
  if the prompt there isn't the one you saw (answered in the pane meanwhile, or
  another command asking). A menu's "Type something." isn't offered: `⏎` opens the
  pane for that. `esc` goes back. The task works again once its agent does, as when
  you answer in the pane.
- **The coordinator asked** (`bullpen ask`): `i` shows the task and the whole
  question, wrapped to the pane, with a line under it, "Answer:" (`⏎` sends, `esc`
  goes back). What you type goes to the task's coordinator, not its worker: typed into its pane as
  `BP-3: <your answer>`, or, with `config --news monitor`, as a news line it reads.
  A task asked about while it was working, in review or ready goes back to that.
  One that was stopped leaves needs as `bullpen ok` would (working, or in review),
  or, with its agent's pane gone, goes to its coordinator ("coordinator on it"). The
  coordinator acts on the answer. With no coordinator running, it's
  refused: `t` tells the worker yourself.

`bullpen prompt`, `answer` and `reply` do the same from a shell.
