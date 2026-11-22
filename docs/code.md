# The code

bullpen's command line is Rust (it was Python until the port of BP-101's
`rust-port-plan.md`, BP-103 to BP-116). The tests are still Python (`tests/`,
docs/tests.md); their fake Rex and fake gh are Rust (`tests/fakes/`, BP-138).

- `bin/bullpen` is the entry everything runs: people, `install.sh`'s link, Lua, the key
  bindings, the board and bullpen's own background jobs. It's a small `sh` script that
  runs the build, `target/bullpen`, or with no build says to run `install.sh`. It forks
  nothing on its usual path, and resolves a link (install.sh's) only when it's run
  through one.
- `target/bullpen` (`cli/src/main.rs`) looks the command up in its table,
  `cli/src/commands.rs`: every command, its group in `--help`, its help line, and the
  function that runs it (`bullpen _commands` prints the table). No command or `--help`
  prints the help, and a name bullpen doesn't have is one line ("Did you mean"), both
  in `cli/src/help.rs`.
- A call for an internal command (`_...`: a job's, or the watcher's from a newer
  `watch.lua`) the build doesn't have, while the build is older than its source (the
  window between an accept's merge and its new build), catches up first
  (`build::catch_up`): it waits for the build that's running (at most
  `BULLPEN_BUILD_WAIT`, 600 s), or builds what's stale, and runs the call again with
  the new build, once. Only when it can't does it say to run `bullpen build`. A
  person's command never sets off a build: a typo gets the unknown-command line.
- `install.sh` builds the command line with cargo itself, every time, before
  `bullpen build` (the command line's own). Every place that runs cargo on bullpen
  runs it through `bin/bullpen-cargo`, the one lookup of a working cargo
  (docs/rebuilds.md).
- The Rust is one cargo workspace (`Cargo.toml`, one `Cargo.lock`, built into
  `target/`): `core/` (`bullpen-core`: tasks, config, wording, what the command line
  and the board share), `cli/` (`bullpen-cli`, the program `bullpen`), `board/`,
  `draw/` (`bullpen-draw`: what the board's image look and the drawn tour draw with)
  and `tests/fakes/` (`bullpen-fakes`, the tests' fake `rex` and `gh`; only
  `tests/run.py` builds it).
  `core` never depends on `cli`; the board depends on `core`, the board and `cli` on
  `draw`. The crates are `serde_json` (with `preserve_order`), `chrono`, `libc` and
  `unicode-width` (the board's too, and ratatui's); ask before adding one. The board has
  ratatui's and its own besides. `draw` has the image look's: `tiny-skia` (drawing),
  `ttf-parser` (fonts) and `miniz_oxide` (its PNGs' zlib), and its fonts in
  `draw/fonts/` with their licences (Inter, and JetBrains Mono for the tour's code,
  both SIL Open Font License; JetBrains Mono is 204 KB of the program). The board and
  `cli` use `tiny-skia` too. A module's Rust tests are in a `tests/` folder
  beside it, as `test_<module>.rs`. rustfmt with `max_width = 120` (`rustfmt.toml`)
  for `core`, `cli` and `tests/fakes`; the board and `draw` aren't formatted yet.
- The files bullpen writes (tasks, config, news, stamps) are as the Python wrote them:
  JSON as Python's `json` (indent 2, ASCII only, keys in order), stamps as `isoformat`,
  cuts as Python's. Comments that say "as Python's" name what's kept. The width of text
  on screen is `unicode-width`'s, as the board and ratatui measure it; it differs from
  Python's `unicodedata` only on zero-width characters, which take no column.

## Commands

A command is a row in `cli/src/commands.rs` (its group is a title in `help::GROUPS`;
internal commands, the watcher's and bullpen's own, named `_...`, have none) and a
function `cmd_...(argv)` in its module, which reads its arguments (`args.rs`, by
argparse's rules: prefixes, `--x=y`, `--`, the last of a repeated option) and calls
plain functions. No other code calls a `cmd_...`. Auto-accept calls `accept::accept`
(cli/src/accept/mod.rs). Its docs are a line in `docs/commands.md` at least (see
[Tests](tests.md#the-tables)); a setting is a row in `SETTINGS` (`core/src/settings.rs`)
and its words in `docs/settings.md`.

A doctor check is a `DoctorCheck` value in the module of `cli/src/doctor/` for its kind
(`tools.rs`, `builds.rs`, `files.rs`, ...), in that module's `CHECKS`: its name, whether
it is about the setup rather than a repo (`shared`: `--all-repos` runs it once), whether
`up` leaves it out (`slow`), how it runs, and its `--fix`, with the fix's words in the
flag's help, if it has one. `doctor/mod.rs` lists the modules in doctor's order and
knows nothing of any one check. Its docs are a line in `docs/doctor.md` naming it.

## Where things are

Each module says what it's for in its own header: the opening `//!` comment of a Rust
file, the opening `--` comment of a Lua one. `bin/code-map` prints them as the code map,
grouped by crate and folder, a line per file (each header's first paragraph):

    bin/code-map                  # the whole map
    bin/code-map cli/src/news     # only the files under these paths
    bin/code-map --full board     # each file's whole header

It reads the checkout it's in, so a worktree maps its own code, built or not. There's no
list to keep: a new module writes its header, and a test (`tests/test_cli.py`) holds
every module in `core/`, `cli/`, `board/`, `tests/fakes/` and `lua/` to having one.
A module's tests (`tests/test_<module>.rs` in its folder) aren't on the map.

## The board's drawing

The board (`board/src/`) draws with ratatui. `ui/rows.rs` works out the list's rows
from the board's state once a frame, as values: each row's text, its mark, whether
it's selected, asked about, dim, its risk and age (and the selected task's first
key, which only the image draws). The text list (`ui/list.rs`, `ui/later.rs`) draws
them as cells. With the image look (docs/board.md) the board leaves its cells blank
and hands image/ a `View`: the same rows, the top line (`list::Head`, with each open
group's count for the status bar: `bar.rs`) and the bottom rows (`bottom::Foot`) as
values too, and the blocks of cells each has (on a tall board the header has the
three rows above the list). After ratatui's frame, and through the same output, image/ lays the
rows out (`render.rs`: each row's height from the text size, the selected title
wrapped; the marks' shapes are its child `marks.rs`, and the keys a line names, in
caps, its child `said.rs`), scrolls them by those heights (`list::scroll_by`, which the text list's
`scroll` is with every row 1 high; it sets `app.top` and `app.page`), and draws the
top line and the bottom rows (`frame.rs`) and the list, all with no terminal. The list
is pieces (`pieces.rs`): a window of the rows drawn with nothing selected (the `View`
has them too, `rows::unselected`; the page when a row on it changed, else two pages at
a page's steps), placed as two crops above and under the selected block, which is an
image of its own; a block is laid out the same selected or not (`ui/rows.rs` gives the
image's selected task the lines it has unselected), so there is one layout, the crops
stay where they are, and scrolling moves them. The window on the terminal stays
while the rows shown are as it drew them (`list::still`). Every piece draws its rows
where they are in the whole list (`render::Cut::at`), so the pieces match the list
drawn whole to the bit. The marks that
move and the scroll marks are small images sent once (`sprite.rs`): a mark on screen
is a placement of its strip of frames, and a tick changes which frame it crops; the
list's images leave those marks out. `list.rs` says which images a frame wants, each
with a key for what it shows and its placements, and `shown.rs` sends, places, moves
and hides only what changed, with the kitty graphics protocol (`kitty.rs`); an
image a frame replaces is kept on the terminal, hidden, to be placed again (bounded
by count, and with what's shown by the pane's budget: see below). An image over the terminal's limit is sent as pieces (see
[Images under the terminal's limit](#images-under-the-terminals-limit)): on a tall
pane the list's window of two pages is several pieces, each placed where its cells
are. While no key is waiting, `ahead.rs` presses j and k on the board,
draws that frame off screen, puts back what the key changed, and sends the images it
would want unplaced, so a j or k only places images; the list's base image it leaves
to a thread of its own (`worker.rs`, its own fonts, answering with `Msg::Ready`),
which also starts on the list's next window as the page nears its edge. The main
loop also draws a key's
frame before it reads the tasks again. While the board is an image, a task's detail is
one row holding its whole text and the most lines it gets (`rows::detail_lines`), and
`render.rs` wraps it by pixels, as its type measures; the text list's are wrapped to
its columns, a row a line. Asking the terminal (`term.rs`) needs its input, so the board asks only when
the image look starts or comes back and on a resize, never on a timer: the key reader
(`main.rs`) stops at the board's wish (a `Gate`), marks itself parked before it tells
the board, and reads the answers itself, up to the terminal's last; the gate opens to
keys again when the reader's `Open` drops, on every way out. What's typed or asked at
the bottom and overlays stay text.

Mission control's image look (docs/mission.md) is the same code: `Picture` draws for a
`Host` (image/mod.rs), which the board's `App` and mission control's `Mission`
(mission/look.rs) both are. Each hands it a `View`, says its look and type, takes back
the list's first row, and lets drawing ahead press j and k on it off screen. Mission
control's `View` is the board's rows (ui/rows.rs) for every repository's tasks, each
from its repository's `App` as its board makes them, with a heading for each
repository (`Row::Repo`, drawn by image/fleet.rs) and the inbox's tasks naming theirs
(`TaskRow::repo`; `TaskRow::who` tells tasks apart across repositories, whose IDs
repeat); its header is `fleet` (image/fleet.rs: the chips and the fleet's bar, a run of
segments a repository, wrapped onto more lines rather than cut a name), as wide as the
pane (`full`), and the selected task's page beside the list is `side` (image/page.rs),
an image only as tall as what it says. The page is drawn ahead with the selected block,
so j and k only place images there too. Its key reader is the board's (`read_keys`,
stopping for the terminal's answers), whose messages run.rs hands on to its loop.

How fast a j or k is, without a terminal: `bin/bullpen-cargo test -p bullpen-board
--release bench -- --ignored --nocapture --test-threads 1` (image/tests/test_bench.rs)
drives the board's own code over a fixed board on disk (every status, long titles, the
Later group, 200 closed tasks), at 48 and 80 columns by 45 rows, 16×34 and 32×68 pixel
cells, each text size, motion on and off, and the text look beside them. For one key
it prints the time from the key to its frame's last byte and where it goes (building
rows, laying out, drawing and PNG-encoding each image, base64, writing; `image/meter.rs`
notes them, in tests only), the bytes sent by image, how many keys sent the list
again, and what drawing ahead took after it; the first frame, and a task's title
changing on screen and off; then a held-down j (20 keys 30 ms apart):
the frames, how far the last frame lands after the last key, and each key's wait.
`BENCH_ONLY="80×45 32px,text"` runs some cases. Mission control's case is `bench_mission`
(mission/tests/test_bench.rs, run the same way by its name): four repositories with a
task in every status and a blocked coordinator, at 160×46 cells of 16×34, 211×65 of 19×37
and 64×48 of 19×37, its text beside its images; for a j or k, the time to its frame's
last byte, the bytes, how many keys sent the list again, and the first frame. Sizes come from the renderer, so it
stays right when the look changes; run it before and after a change to the look's
drawing to see it didn't undo these numbers. It can't see the terminal's own decoding
and drawing, or the network.

The board's image look draws with `draw/` (`bullpen-draw`): the type (`font.rs`; its
glyphs' strokes put on whole pixels by `hint.rs`, a light auto-hinting: stems and bars
found from the outline's edges, each a whole number of pixels, the baseline, x-height
and capitals' height on rows, the rest of the outline moved straight between them; a
bold the font lacks is its regular made heavier before it's fitted, as FreeType's
embolden does),
shapes, the terminal's colours and their tones (`theme.rs`; a flag's colours stay the
board's, `image/theme.rs`'s `Flags`), PNG, the kitty protocol's bytes, cutting an image
under the terminal's limit (`cut.rs`) and the terminal's answers to its questions
(`answers.rs`; reading them, with the key reader's gate, stays `image/term.rs`).

### Images under the terminal's limit

Rex's app draws a kitty graphics image of at most 10,000,000 bytes decoded (width ×
height × 4) and drops or blurs a bigger one, while the server still answers OK (Rex's
bug report "Kitty graphics: inline images of 10 MB or more are silently dropped by the
client", 21 Oct 2026). A pane's worth of pixels is more than that on a large pane: the
tour's pages were sent in tiles a window high, about 29 MB each at 217×65 cells of
19×37 pixels, and the board's list window of two pages is about 30 MB at 77×72. So no
image either program sends is over `cut::MAX_BYTES` (9,500,000, a margin under the
limit). `cut.rs` cuts an image over it into strips its whole width, as tall as fit, each
overlapping the next by a cell (and into pieces side by side too, should even a strip
two cells high be over), and splits a placement of the whole image into its pieces'
placements, a run of whole cells each. The overlap keeps every cell of a crop inside one
piece even when the crop doesn't start on a cell's edge, as the board's list doesn't
when its rows aren't whole cells; its tests check that the pieces placed are the image
whole, at every crop. The board's `shown.rs` and the tour's keep each image's pieces
under its ID and place, hide and delete them together. The tour draws its pages in
strips that fit to begin with (below). `bin/kitty-limit-check`, run in a Rex pane,
places an image under the limit beside one three times over, to see what the terminal
does with the second.

Rex's terminal also keeps only so many bytes of images in a pane: 128 MiB decoded
(`cut::QUOTA`). An upload past that deletes the oldest images not on screen, and with
q=2 nothing says so, so an image kept hidden to place again was simply gone and its
place blank (going back a step in the tour, at 211×65 cells of 19×37 pixels, its rail
and first strip). `bin/kitty-quota-check` finds the quota: it sends 60 images of 9.5 MB
as the tour does, then places each asking for an answer; Rex kept the last 14 (133 MB)
and answered "ENOENT: image not found" for the rest. So the board and the tour each
keep at most `cut::BUDGET` (96,000,000 bytes, about three quarters of it) on the
terminal, shown and hidden, and delete the oldest they keep themselves before sending
what would pass it: the terminal never deletes one for them, and what they haven't
deleted is there to place. What they have is sent again when it's wanted. The quota is
a pane's (each screen's, as libghostty's): the tour's floating pane and the board's
are counted apart.

Every placement is pinned to cells: its crop is the cells' size in pixels, as the pane
reports them (its pixel size over its cells, rounded down), and the terminal scales the
crop to the cells it draws. So an image must be exactly its cells' pixels at the cell
the pane has now: then Rex shows it pixel for pixel, as sharp as its own text, and one
even a pixel over (a scale of 0.9985) is visibly resampled, its one-pixel patterns
grey. `bin/kitty-sharpness-check` shows this: the terminal's own line of code over the
same sample placed three ways (pinned as bullpen pins, pinned with the image a pixel
over its cells, and unpinned), with `--float` in a floating pane as the tour's. The
sample (`bullpen _sharpness_sample`, `look/sample.rs`) is a line of code drawn as the
tour draws it, one-pixel lines and checkerboards, white on black. On a Retina Mac at
19×37 pixels a cell, pinned and unpinned were alike and sharp.

Rex opens a pane at 80×24 cells of 8×16 pixels and sizes it a moment later (the tour's
floating pane, just opened, says that size), so a cell read once at the start can be
wrong for good: drawn at 8×16 and stretched about 2.3 times to the real cells, the tour
was big, bold and blurry. The board reads its cell from the window every frame (its
images' keys have it) and a resize draws it again. The tour reads it every time round
its loop too: a new cell deletes every image and page height kept and draws anew
(`Look::set_cell`). Opened at that unsized size, the tour is text until the pane is
sized (`look/ask.rs`'s `UNSIZED`, 1.5 seconds at most, `view.rs`'s `SETTLE`).

## The tour's drawing

The tour (`cli/src/stepper/`, docs/tours.md) keeps its state and keys in `screen.rs`'s
`Stepper`, whichever way it's shown. `view.rs` runs it on the terminal: with
`board_look image` it asks the terminal first (`look/ask.rs`, `draw`'s questions), and
when the terminal draws images it shows the stepper with `look/` instead of as text.
`look/mod.rs` lays the pane out (`Lay`: the rail's columns, the page's, a tile's rows and
the window's) and draws a frame: the rail (`rail.rs`, every step's title, the step
shown in blue), a window on the page, and the foot (the message and keys); what's typed
is text where the foot was. The type's sizes come from the cell (`ink.rs`'s `Sizes`):
code at the em where its advance is a cell, drawn a character a cell on whole pixels
(`Ink::cells`) and a line a row from a row's top; prose from the cell's height as the
board's medium or large type. A page (`page.rs`: a step's title, files, note, comments and
code, `code.rs`; or the end) is measured once, on an image a pixel high (its rows are
kept), and sent as tiles: strips under the terminal's limit at the page's width, the
window's rows shared evenly among as few as fit (`Lay::fitted`), each drawn on an image
the tile's height with the page shifted up by its top (`Ink`'s window: code outside it
isn't drawn), so no image is bigger than the limit however long a step is (`shown.rs`
sends each image once, places crops of it, and keeps it hidden to place again; the rail
and foot are cut when they're over the limit). An image it can't make is an error, and
`view.rs` turns the tour to text, saying why. The window is crops of the tiles it
covers, so a scroll is placements. While no key waits, `Look::ahead` sends what `j`,
`k`, `n`, `p` and a page down would want next, unplaced, an image at a time with a key
looked for before each, so a key waits behind one strip at most. It sends only what
fits in the budget with what's on screen, likeliest first, and holds it (`Shown::begin`)
so sending one never deletes another it wants; `shown.rs` deletes the least recently
wanted of the rest for room. Its tests (`look/tests/`) check the bytes each key writes
and that no image is over the limit at a 300×100 pane, and play `n`, `p` and a long
step paged down and up into a fake terminal with Rex's quota (`tests/terminal.rs`),
checking every image placed is there; `bytes_at_large_panes` and `bytes_going_back`
(ignored) print the bytes at 160×46 and 217×65, and of those keys at 211×65;
`render_tour` (ignored) writes pictures of a tour fixture (`TOUR_FIXTURE`,
`TOUR_RENDER`).

## Closing a task

Accept, close, drop, finish's auto-accept and auto-accept end a task through one close
step, `close` in `cli/src/close.rs`, in one order: the reviewer's pane, the PR
(withdrawn, unless it was merged), the worktree (and its Claude scratch folder) and branch, the status and the save,
`bullpen: started` taken off an issue's task's issue when nothing was merged (issues/write.rs),
then what the caller says and does (`then`: news, and after an accept the rebase of
the other tasks), and the worker's pane last, since the step may run in it. Each
caller says only what to tell and how to merge. Nothing calls the run queue: the save
moves it.

## Making a task ready

`ready` (review off, or a branch with no commits), an approval kept at `ready`, the
verdict (an approval, or findings after the last round) and a worker's turn that leaves
a ready task's code as approved (`ready_again`) make the task ready through one step, `make_ready` in `cli/src/become_ready.rs`, told how it
became ready (`How`): the risk label, the status, the save, then auto-accept (only for
an approval, as a job started one way, `start_auto_accept`) or the ready news. A branch
with no commits is recorded there as `nothing_to_merge`, the field core's
`nothing_to_merge` reads, and it goes when the worker is back at work (`back_to_work`)
or runs `ready` again; a task made ready before that field is read by its why. A
verdict's approval is checked against its base first when accepts left it in review
(`left_in_review::after_approval`), before auto-accept or the news; the verdict waits
for its auto-accept, the others don't. A ready task rebased after an accept with its
approval kept (`rebase_task`) goes through it too, as `Rebased`: labelled for its code
now, with neither auto-accept nor ready news (the accept's `After BP-3: ...` line says
it). No other code sets a task ready (`test_become_ready.rs` holds that); `answer`
only puts back the status a question interrupted.

## Where a pane lives

A worker's, reviewer's or diff's pane goes where its placement in `cli/src/panes/` puts it:
`FLOAT` (a capsule in its coordinator's tab, the default), `TAB` (a capsule in its
coordinator's workers tab, found by the tab's label), `TABS` (a tab of its own) or
`BACKGROUND` (a detached Rex block, in no tab), picked by the task's own `placement`
(`start --pane`) over the `workers` setting, and the `reviewers` setting (`for_new`; a
diff never goes to the background). Rex places a block only beside another, and one
placed beside a floating block joins its layer, so `gather` moves a pane into a workers
tab beside a stand-in on a layer (or in a tab) of its own, then closes the stand-in. Each placement has the same
steps: `place` (make the pane; one the hide key puts away gets its hint in its label),
`show`, `hide` (False when it has no away, so `hide` exits 3), `find` (by ID, else
label, after a Rex restart), and what `gather` does to keep it home. Code that makes,
shows, hides or finds one of these panes calls `panes.place`, `show`, `hide`, `find`
and `gather` (a worker's or reviewer's is made through its host, below, whose Rex
side calls `place`), and never reads the settings. `place` returns the placement's name with
the block, and the caller records it with the pane (the task's `pane`, its review's
`pane`): a background pane that's up floats like a capsule, so the record is what
says to detach it rather than minimize it. Otherwise showing and hiding go by where the
pane is now (`placement_at`), so a pane started before the settings changed keeps
working. Rex's `session.view` lists only placed blocks: `BACKGROUND.find` asks
`session.list_blocks` for the detached ones, once per view. A new placement is a new
`Placement` there.

## The host an agent runs on

A task's worker and reviewer, and a coordinator, are agents on a host: Rex now, where
an agent is a pane. What bullpen asks of a host is the `Host` trait in
`cli/src/host/mod.rs`, in its own terms: start an agent for a task (with a label, in its
worktree), tell it something, is it alive, read the end of what it shows, its status
record and its state (working, blocked, idle, a shell still running, gone), is it
compacting, the question it's at and answering it, is something half-typed in its
input, show, hide and close it, and set its title. Code reaches an agent through an
`Agent` (its host, where on the host it lives and its ID there): `Agent::of(record)`
for a task or coordinator, `agents::agent(t, who)` for a task's worker or reviewer.
Nothing there assumes a pane or a screen: Rex reads its screens (`screen/`) to answer
what another host may report itself.

A task records its agent's host as `host` beside its `session` and `block` (its
review's, beside the reviewer's), written by `Agent::record`; a record without one is
on Rex, so older task files read as they did. `host::for_new` picks the host a new
agent starts on (always Rex for now).

The rest stays Rex's and calls `cli/src/rex.rs` itself: panes and their placements
(above), boards, diff panes, the watcher, mission control, `go` and the hide key (they
move between Rex panes, boards included), finding panes again after a Rex restart
(`go/relink.rs`), starting a coordinator (`up`), notifications sent through a pane's
terminal, and kitty graphics. `host/mod.rs`'s header lists which parts are which.

To add a host: implement `Host` in a file of its own in `cli/src/host/` (Rex's is
`rex.rs`), add it to `host::named`, and have `for_new` start agents on it when it
should. The Rex-only code above goes through tasks' blocks (gather, relink, the block
index), so a host whose agents aren't Rex panes keeps its tasks out of it.

## Local mode and remote mode

The steps that differ between the modes go through one of two adapters in
`cli/src/modes.rs`, `REMOTE` (origin and gh) or `LOCAL` (the local base branch and the push
remote), with the same verbs: `publish` (at ready), `merge`, `after_merge`, `withdraw`
(close and drop), `refresh` (push a branch bullpen rebased) and `base_ref`.
`mode(repo, t)` picks one: the mode recorded on the task when it was published (its
`local`, true or false), else the repo's as it is now. Code doesn't ask
`repo.has_origin()` about a task; that's only for the repo itself (doctor, the push
remote, the base new work starts from). Every push goes through `quiet_git`, which
never lets git wait on a prompt. It runs git through core's `proc::quiet` (no terminal,
a session of its own, a timeout), as `issues/repo.rs`'s `gh_quiet` runs gh for every
issue read and write.

## Start-up

Agents, hooks, the board and the watcher call bullpen all the time, so a call stays
cheap: the `sh` entry adds about 0.5 ms, and a command takes about 4 ms (`_nop`
times start-up). A command reads only the files it needs.

## What a save sets off

Saving a task goes through `saves::save`: core writes it (`Repo::save_task`, which
returns the task as it was), then, in one order, the run queue hears of the status
change (`move_queue`: a task that leaves `working` or closes frees a slot), the
person's notification (`notifications::notify`), and a PR the watcher should follow
(`follow_saved`). No caller has to remember any of them.

What a save sets off that should wait for the command's own work (the run queue,
whose news follows the command's) goes to `saves::later(key, fn)`. `main` runs it when
the command returns (`run_later`), and a command that closes the pane it runs in runs
it first: `rex::close_last` closes a pane after it.

A file stays under 1,000 lines unless there's a good reason; a test
(`tests/test_cli.py`) holds every `.rs` file in `core/` and `cli/` to it.
