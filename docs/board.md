# The board

The task board (`bullpen board`, a Rust program in `board/`) sits in a narrow
pane beside the coordinator and updates as soon as a task changes.
It groups tasks by what you do next: needs you, coordinator on it (dimmed: see
[Triage](triage.md)), ready to accept, finished (nothing to merge, or a branch to
keep unmerged, see [Finish](finish.md)), in review, working, queued, and last the backlog's ideas
(Later, see [The Later group](#the-later-group)); `f` adds
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
| `◆` green | finished: nothing to merge, or a branch to keep |
| `◜` blue, turning slowly (`◐` still) | in review |
| `⠋` turning slowly (`○` still) | working |
| `◌` dim | queued, not started |
| `·` dim | accepted or closed |
| `–` dim | dropped |
| `⣾` turning, in place of the mark | a command (accept, drop, start, resume) or its tour being written |
| `▌` at the left edge | needs you (amber), stopped (red), or no progress (amber) |
| `↻` dim, after the mark | the task came from the backlog |

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
`ready`. A ready task has its risk before its age, a word in its colour (`low`
green, `medium` amber, `high` red), and why at the end of its sentence ("High risk:
changes how accept merges."; see [Review](review.md#the-risk-label)).

The top line counts only what you act on, at its right: `2 need you · 3 ready`
(need you: needs or stopped; ready: ready to accept, or finished), in amber and
green, shortened to `●2 ✔3` when the words don't fit. Before it come the other
repositories with tasks that need you or are ready (see [Several
repositories](several-repositories.md)), which get the room first. On a crowded
line the hidden tasks' note drops its "o shows" first, then this repository's
counts shrink to marks, then to what needs you alone; the other repositories go
only when even their marks don't fit.

`bullpen ls`, `bullpen show` and the picker (`cmd+;` `j`) say a task the way the
board does: its mark (from the table above, with the `↻` of a task from the backlog), its ID, title and age, then the
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
| `p` | peek at its worker's screen (in review, its reviewer's, as `⏎` opens), in its colours, updating until `esc` or `q`; `t` `⏎` `i` `a` `d` `v` in a peek go back to the board and act on that task |
| `v` | view its diff |
| `g` | its guided tour (see [Guided review tours](tours.md), with the keys inside a tour) |
| `s` | start it |
| `r` | resume a stopped worker |
| `l` | switch layout: compact, one line a task, or back |
| `I` | draw the board as images, or as text again, for as long as the board runs; switching to the image asks the terminal's colours again (see [The image look](#the-image-look)) |
| `f` | show the closed tasks too |
| `o` | show other coordinators' tasks too (see [Several coordinators](several-coordinators.md)) |
| `esc` | put a red message away (see below) |
| `?` | every key, one short line each, and the marks |
| `q` | quit the board, once you say `y` (`ctrl+c` quits at once) |
| `L` | the Later group: show its items, or fold them back to its line; its items' keys are under [The Later group](#the-later-group) |

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

## The image look

`bullpen config --board-look image` (or `I` on the board, for this run) draws the
board as images instead of text: real type, the marks drawn as shapes (rings, a
check, a diamond) in the status colours, all the same weight to the eye, each one's
left edge where a group's name starts. The list says everything the text list does,
row for row: each task's ID, title, mark, risk, age and lines, the groups and the
Later group.

One grid holds it: text starts a column and a quarter in from the pane's left edge
and ends that far from its right, the rules and the status bar run between the two,
and a card or a wash reaches three quarters of a column past each, so type is clear
of their edges and they're as far from one side of the pane as the other. The ages
have a column of their own at the right, two figures wide, so the risks' chips before
it line up down the list; a title's lines after its first stop short of it. A task
is one block with a hanging indent: its mark and its ID are in a gutter at the left,
and its title, every line the title wraps to and every line under it start at the
title's left edge. On a board under 44 columns the ID is a size smaller and the gap
after it narrower, so the gutter takes less of those lines.

The selected task sits on a card, and nothing else is boxed: a faint fill and an
edge three pixels wide in the terminal's blue, its mark inside it, its ID in the
foreground and its whole title (wrapped to three lines at most). At its lower right
is its first key in a small cap with what it does, `a accept` or `i answer`: its
last line makes room when it must (its last words go on a line more, or it's cut
when it has none), and the lines above keep their width. A task that needs you, or
whose agent stopped, stands out: it sits on a faint wash of amber (or red) over the
card's rectangle, with a bar of that colour down its left edge, following its
corners; selected, its fill is that colour stronger and the blue edge stands in the
bar's place. An age that has waited too long is a chip of its task's colour, whole.

A title is the loudest thing on a row: a task's ID (dim, on the title's baseline)
and age, and the groups' names (a little dim), are a detail line's size at most. A title
that's yours (it needs you, stopped, or is ready) gets two lines, at medium and
large; another's is cut to its line, and is the foreground a step back (the
coordinator's, and a closed one's, a step further), so yours lead the list. A cut is
after the last whole word that fits, never after a comma or a full stop (in a word
only when that would leave under half the room, as with a path). A detail line wraps
where the type runs out of room, measured in pixels, and its last line isn't left a
word or two; it's dim, but the selected task's and what a task that needs you says.
A key a line names ("Its agent exited. `r` resumes it.") is in a small cap, as the
keys at the bottom. A ready task's risk is a quiet chip: a dot of its colour, then
the word dim, inside a faint line (on a board under 44 columns, its first letter: H,
M, L). A group's name sits with its own tasks: there's more room over it than under.

The marks move as the text board's do, at the same pace: a working task's is a faint
ring with a quarter of it going round, a task in review's a blue ring with a filled
half that turns, and a running command's (or a tour being written) half a ring
turning faster. When a task's status changes, its new mark settles in: it grows a
little past its size and back, in 160 ms. `bullpen config --board-motion off` stills
them all (the working, review and running marks stand as their first frame), and the
board then sends nothing while nothing changes.

The top line and the bottom rows are drawn too, in the list's type and colours, each
set off from the list by a faint rule: the repository's name, the pane's largest
type, then its counts at the right as chips ("3 need you", or only their numbers
when the words don't fit beside the name), a word in its colour on a round pill of
that colour, faint. On a board tall enough for the text's blank row under the top
line, the header takes that row too for a status bar, in the rule's place: a segment
for each open task, in its group's colour and the board's order (red for a task
whose agent stopped and amber for the others that need you, a violet for the
coordinator's, green ready, a line of green round a faint fill finished, blue in
review, greys for working and queued), and under it each group's count ("● 3
ready") but what needs you, which the chip says: as many as fit, then "+N more" for
the tasks of the rest. The list doesn't lose a row for it: the text's ↓ row under
the list goes. With the bar, the chips on the line are only what needs you; the
counts under the bar say the rest. The header is sent again only when a count
changes, so the bar costs nothing while you move around. Then the message, and the
keys each in a small rounded cap with what it does dim after it, dropping from the
end on a narrow board as the text's do (`?` is never dropped); they sit a little up
in their rows, clear of the pane's bottom edge, and the rule over them is above the
message's row whether there's a message or not. The scroll marks are small chevrons
at the list's right edge. What you type into stays text: while a tell, a y/n
question (an accept's, a drop's, `q`'s) or the stale tour's question is open, the
bottom rows are text, cursor and all, and are drawn again after. `?`, peeks, answers
and mission control stay text, and so does the compact layout (`l`): with it the
board is text whatever the look.

`bullpen config --board-text-size` sets the type: `medium` (the default) is the
terminal's own text size, with a detail line smaller and dim under each title, a
little space between them and about three quarters of a line between the tasks, so a
row is a little more than a cell (cards one under the other are a few pixels apart); `large` is larger still, for a pane read from
further away, and cuts titles sooner; `small` is a cell a row, as the text list. The
list scrolls by its rows' own heights, keeping the selected task and its lines in
view and showing only whole rows, so a page (`PgDn`) has fewer tasks than the text
list's.

It needs a terminal that draws kitty graphics, as Rex does. The board asks the
terminal first and draws text until it answers; when it doesn't draw images (or
doesn't answer in two seconds), doesn't say its cell size in pixels, the font
can't be read or drawing fails, the board is text again and the message line says
why ("Drawing the list as text: the terminal doesn't draw kitty graphics."). `I`
tries again.

The image follows the terminal's theme: its colours are the terminal's own
foreground, background, red, green, yellow and blue, and its background is clear, so
the pane's shows through. Text is in those colours as they are; a shape (a mark, a
bar, a dot) is in the colour lifted when it's dark, and a fill (a wash, a chip's) in
the colour at its brightest, faint, since a light theme's amber is dark to read as
text and would be brown as a shape and beige as a fill. The board asks for them (with the cell size) when it starts
drawing images, and again only on a resize or when you switch to the image look: after
a change of theme, `I` twice redraws in the new colours. It never asks on a timer.
While the board waits for those answers it reads the keys itself and passes them on;
an answer that comes late (after two seconds), however late, is read to its end and
never taken for keys, and an `esc` typed meanwhile counts once nothing follows it for a
tenth of a second (an answer that was cut right after its own esc gives the board that
`esc`, never the rest as keys). The type is Inter (built into the board, SIL Open Font
License); `bullpen config --board-font PATH` draws with another font file (TTF or
OTF) instead, with Inter for the characters it lacks.

The board is images: the top line, the list's rows shown and the bottom rows, each
drawn at the terminal's cell size and sent only when what it shows changes (a task's
status or age on screen, the message), so a change to the list doesn't send the top
line again. The list is drawn with nothing selected, and the selected task's block
(its card, its whole title and all its lines) is a small image of its own over the
cells it takes: the rows under it move down by whole cells to make room, by placing
the list's image again lower, not sending it. The list's image is a window of the
list: the page, when something on it changed; scrolled out of that, two pages at a
page's steps, so scrolling a row places it again higher and sends nothing. It's sent
again when a row on the page changes (a row off the page that changed is drawn when it
comes on), or when the list scrolls out of the window. While the board waits for a key
it draws what `j` and `k` would show next (the block a task down and up, and the key
row for it) and sends it without showing it; the list's next window, when the page
comes near the window's edge, is drawn on a thread of its own, so a key doesn't wait
for it. An image a key replaces is kept on the terminal, hidden, a while (the last 16,
and one of the list's). So `j` and `k` send only a few placements, a few hundred
bytes, and draw nothing. The board draws a key's frame before it reads the tasks again
(then reads them, and draws what changed), so a key doesn't wait for the task files.
Everything is deleted when the board is text again or quits. The marks that move and
the scroll marks are small images too, sent once: a tick only places another frame of
a mark (a few dozen bytes a mark), never the list. The kitty protocol's own animation
(the terminal turning the frames itself) would send nothing at all, but Rex's support
for it isn't reliable yet, so the board doesn't use it. Each image is pinned to its
block of cells, so a client with other cell sizes (Rex on a phone) scales it to fit.

`bullpen-board --dir <state dir> --root <repo> --bullpen <program> --render FILE
--cols N` writes the board as the image look draws it to FILE: its top line, its whole
list (as tall as its rows, not rounded up to cells as on a terminal) and its keys, a
PNG N columns wide (60 by default), at the repo's text size, without a terminal. `--theme light` draws it in a light terminal's colours (GitHub's
light ones) on white, `--theme dark` (the default) in the default dark ones, and
`--select ID` selects that task, so a change can be checked on both.

## The Later group

The backlog's open items (see [The backlog](backlog.md)) sit last, under Queued (and
above Closed, with `f`), folded to one line:

```
Later · 6 · next: Tidy the watcher's error messages
```

In amber when something there waits on you, a worker's idea or a
[GitHub issue](issues.md) asking for a yes:
`Later · 6 · 2 to decide (L8, L9)`. An empty backlog has no line. `L` shows a row per
item, in list order, and selects the first: `?` in amber for one that asks, `◇` dim for
a kept one, then its ID and title; the selected one has a line saying what to know
(who it asks from, why the coordinator skipped it, whether it has a brief) and the key
line its keys. `j` `k` move between tasks and items as one list; `L` again folds it,
back on the last task. Once the last item goes, the group folds by itself: the next
item added shows as the one line again. On an item:

| Key | Does |
| --- | --- |
| `y` | keep it: an idea that asks goes on the list (`bullpen later L8 --keep`) |
| `n` | drop it (`--drop`), once you say `y` to `Drop L7 "Tidy the errors"?`, red above the key line as a task's drop is; the next item is selected |
| `s` | start it: one with a brief becomes a task and starts, in the background, and the new task is selected (`--start`). One with no brief can't start yet: the board asks the coordinator to write one and start it (typed into its pane, as `bullpen say` does) and takes you there, as `c` does |
| `J` `K` | move it down or up the list (`--down`, `--up`) |
| `⏎` | the item in full (`bullpen later L7`) |

A task started from the backlog has a dim `↻` after its mark, here, in `ls` and in the
picker, and its line ends "From the backlog (L7).".

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
  `BP-3: <your answer>`, or, with news that isn't typed (`config --news wake` or `monitor`), as a news line it reads.
  A task asked about while it was working, in review or ready goes back to that.
  One that was stopped leaves needs as `bullpen ok` would (working, or in review),
  or, with its agent's pane gone, goes to its coordinator ("coordinator on it"). The
  coordinator acts on the answer. With no coordinator running, it's
  refused: `t` tells the worker yourself.

`bullpen prompt`, `answer` and `reply` do the same from a shell.
