# Guided review tours

A larger change reads better in order. `bullpen tour BP-3` asks a cheap agent for a
**tour** of a ready task (or one in review): the change in reading order, one
**step** per logical change, each with a title, a note on why it changed and what
to look at, and the hunks it covers. You step through it on the board (`g`),
comment on steps, then accept or send the comments back. The design is in
[docs/prd/guided-review.md](prd/guided-review.md).

**On the board**, `g` on a ready task (or one in review) opens its tour in a large
floating pane in the middle of the window, the way the jump picker opens. With no
tour yet (or a failed one), it starts the agent: the task gets a spinner, and the
tour opens by itself once it's written. With a stale one, the board asks: `n`
writes a new one, `o` opens the old one anyway. On any other task it says why not.
In the tour:

- Each step shows its title, the files and lines it covers, its note and your
  comments on it, then its hunks from git (through delta when it's installed).
  `n` and `p` (or `→` `←`) go to the next and previous step, `j` `k` `space` `b`
  `g` `G` scroll, `esc` (or `q`) closes it back to the board.
- `c` comments on the step: type it and press `⏎` (`esc` drops it; `ctrl+j`
  starts a new line; the other keys are in [Typing a line](board.md#typing-a-line)). To point at
  a line, mention it in the text. Comments are drafts, kept with the task, and
  opening the tour again resumes at the step you were on.
- After the last step, the end lists your comments: `s` sends them back to the
  worker (`tour --send`), `a` accepts the task (warning first, if comments are
  unsent: `a` again accepts anyway), `esc` closes.
- As on the board, `t` tells the worker something now (type it, `⏎` sends), `a`
  accepts from any step (a second `a`, since you haven't reached the end), and `⏎`
  and `v` close the tour and open the task's pane or its diff (as text, to copy from,
  also when the tour is [drawn](#drawn-as-images)).

The line with the size of a ready task's change says "Toured" once you've opened
its tour (until a `ready` changes the code), else "Not toured yet" or "Tour written,
not opened yet": a nudge only, accepting never needs a tour. A failed tour shows as
"tour failed: <why>" on the task.

```sh
bullpen tour BP-3             # with a tour, step through it (off a terminal: its path); else start writing one
bullpen tour BP-3 --wait      # write it now, then step through it (or print its path)
bullpen tour BP-3 --path      # its path, even on a terminal
bullpen tour BP-3 --new       # a new tour, even though there is one
bullpen tour BP-3 --show      # its state (writing, failed, current, stale or none) and steps; --json
bullpen tour BP-3 --step 2    # step 2's hunks, from git, as a patch (--json: with its title and note)
bullpen tour-comment BP-3 2 "Line 40 should log the error."   # a draft comment on step 2
bullpen tour-comment BP-3     # the drafts (--json); --remove N removes one
bullpen tour BP-3 --send      # send the drafts back to the worker
bullpen tour BP-3 --view      # step through it on this terminal (the board's g: --pane)
```

- **Only on request**, and only for a task that is ready or in review: a working
  task's code is still changing. `tour` starts the agent in the background (with a
  note on the task that it's being written, for the board) and returns.
- **The agent** runs headless (`claude -p`) in the task's worktree, on `tour_model`
  (`config --tour-model`, sonnet by default; `--global` for every repository). Its
  command is `tour` in `config.json` (`claude`). It gets the diff against the base,
  the brief, the worker's summary and the review findings, may only read files and
  run `git diff`, `log` and `show`, and answers with the tour as JSON: bullpen writes
  the file. It doesn't count toward `max_running`.
- **Its answer.** With `claude`, the answer's shape is given as a JSON schema
  (`--json-schema`), and bullpen takes Claude Code's structured output. From another
  agent (or text), it reads the text as JSON, else the last JSON object in it that is
  a tour (an answer in a code fence, or one corrected after a first try), and notes
  the extra text in the log. An answer that still isn't a tour is asked for once
  more, both attempts in the log, before the tour fails.
- **The tour file** is `briefs/<ID>.tour.json`: the commit and patch-id it was
  written on, and its steps, each `{"title", "note", "hunks": [{"file", "lines":
  [start, end]}]}` (lines in the new file; a binary file has none). Hunks that
  match nothing in the diff are left out, and any hunk no step covers goes into a
  last step, "Other changes". Code is always shown from git (`--step`), never from
  the agent's text.
- **Current or stale.** A tour stays current while the patch is the same: the same
  commit, or the same patch-id after a clean rebase, as review approvals carry over.
  Otherwise it is stale: `tour` says so and shows the old one, and `--new`
  writes another.
- **A failed tour** (the agent failed, or its answer isn't a tour twice) is a line on the
  task on the board, "tour failed: <why>", with the agent's output in
  `briefs/<ID>.tour.log`. No news, no triage: `tour` again retries.
- **Comments** are drafts per task and step (`briefs/<ID>.tour-comments.json`), each
  keeping its step's title, files and lines. `tour --send` writes them to
  `briefs/<ID>.tour-<n>.comments.md`, one section per comment with its step, files
  and lines, tells the worker to read it and address each one, and puts the task
  back to working (closing a reviewer that was on it). The coordinator gets a news
  line for information: `BP-3 sent back by the person after the tour: 2 comments`.
- **Their review.** The worker's next `ready` gets a review of the person's comments
  (with review on): the reviewer gets the comments and the diff since they were sent,
  and checks that each is addressed. It keeps the round it follows, so it doesn't count
  toward `review_rounds`, and its findings always go back to the worker; a later round
  follows up on them as usual. An approval doesn't carry it over: the comments are
  reviewed even if the code didn't change.

## Drawn as images

With `bullpen config --board-look image` (the board's [image look](board.md#the-image-look)),
a terminal that draws kitty graphics shows the tour drawn, in the board's type and the
terminal's colours, so it follows the Rex theme:

- **The rail**, down the left: the task, then every step by its title down a line, the
  steps behind you filled, the one you're on in blue, the ones ahead as rings, and a
  count of your comments beside each step that has some. The last stop is the end. A
  long tour's rail scrolls to keep your step in view; a pane under 80 columns has none.
- **The page**, at the right: the step's title, the files and lines it covers, its note
  (code in it set in a monospace on a faint chip) and your comments on amber, then its
  code drawn: a strip per file with its +/− counts, each hunk under the lines it covers
  and the function it's in, each line with its number, added and removed lines on green
  and red washes. Code is in JetBrains Mono (built in), coloured by a small highlighter
  in the terminal's colours (keywords, strings, comments, numbers, type names); a line
  too long for the page is cut with "…" in its last column. After the last step the page
  is the end: your comments under their steps' titles, and `s` (send them back) and `a`
  (accept) as large keys.
- **The foot**, under the page: the message (or "↓ more" while the page goes on) and the
  keys, in caps. What you type (`c`, `t`) is text there, as in the text tour.

Its type is the terminal's size. Code is on the terminal's own grid: drawn at the
size where a character's advance is the pane's cell width (unless that would be too
tall for a row), a character a cell (two for a wide one) and a line a row, each glyph
starting on a whole pixel and its strokes put on whole pixels (as the board's type is),
so it reads as sharp as the text beside it; a file's strip and a hunk's line are a row each, and
the line numbers are on the grid too. The rest is sized from the cell's height as the
board's [image look](board.md#the-image-look) sizes its type, at medium or larger
(`board_text_size large` is larger; `small` is medium here): the note, your comments
and the rail's steps at the board's title size (0.8 of a cell's height), the files
line and the rail's task at its detail size (0.7), the foot's keys at its label size
(0.6), and the step's title a step above (1.15). The rail is about a quarter of the
pane (24 to 48 columns) for its titles at that size. The cell is the pane's as it is now:
read again as the tour runs, so a pane resized (or a floating pane Rex has only just
sized: until then the tour is text, for a second or so) has the tour drawn anew at its
cell, never stretched from another. Everything you read is at 4.5:1
or more against what's under it, on dark and light themes; the line numbers are quieter.

The keys and what they do are the text tour's. The drawn code can't be selected or
searched: `v` opens the task's whole diff as text (in delta when it's installed), to
copy from or search. `board_font` sets the prose's font, as on the board.

It keeps a remote link cheap the way the board does. A page is sent in tiles, strips
of as many rows as fit in an image Rex will draw (Rex's app draws none over 10,000,000
bytes, width × height × 4: its bug report "Kitty graphics: inline images of 10 MB or
more are silently dropped by the client", 21 Oct 2026; bullpen keeps every image under
9,500,000), the window shared evenly among a few of them, each drawn on its own, so no
image is over the limit however long a step. The window is placed as crops of them, so
`j` or `k` sends a few placements (about 160 to 320 bytes), and a tile only the first
time the window reaches it. While no key waits, the tour sends what the next key would
want, unplaced, one image at a time, looking for a key before each: the tile under the
window and the one above, the next step's rail, first window and foot, a page of tiles
each way, and the previous step's rail, first tile and foot. So `n`, `p` and a page
down usually only place images (a few hundred bytes). At 217×65 cells of 19×37 pixels
the first frame is about 425 KB, drawing ahead after it about 730 KB in images of at
most about 115 KB, and a step not drawn ahead about 400 KB; at 160×46 (17×34 pixels)
about 250 KB, 465 KB and 225 KB. Images are kept on the terminal, hidden, to place again
(the step you go back to) and deleted when the tour closes.

On a terminal that doesn't draw images, or with `board_look text`, the tour is text, as
above, the message line saying why when the image look was asked for.
