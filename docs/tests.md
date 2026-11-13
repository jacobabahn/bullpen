# Tests

```sh
python3 tests/run.py                  # builds the command line, then the whole suite, across CPUs: about 78 s on 16
python3 tests/run.py -j 8             # on 8 processes instead
python3 tests/run.py tour local_mode  # only tests whose module, class or name has one of these
python3 tests/run.py --slowest 20     # and list the 20 slowest
python3 -m unittest discover -s tests # the same tests one at a time: about 9 minutes (build first)
cargo test --workspace                # the Rust tests: core's, the command line's and the board's
```

The tests run `bin/bullpen`, which runs the Rust build (`target/bullpen`). So
`tests/run.py` builds the command line first, at low priority, when its source changed
since the last build (it writes the build's stamp, as `bullpen build` does), and stops
with cargo's error if it doesn't build. A test module run any other way
says so when the build is missing or older than its source: run `tests/run.py`, or
`bin/bullpen build`. The board isn't built for the tests: they give bullpen a fake one
(`BULLPEN_BOARD`). Nor is this checkout, by anything a test runs: `tests/support.py`
sets `BULLPEN_NO_BUILD=1`, so bullpen never builds, or takes the build lock in, the
checkout it runs from (docs/rebuilds.md). The tests about building build a copy of
bullpen, with a fake cargo (`tests/test_board_build.py`).

The test processes run at low priority (nice 10), as does everything they start, so
several agents can run the suite at once and the machine stays responsive. They use
every CPU, and give way when anything else wants one.

There's a module per area (`tests/test_tour.py`, `tests/test_news.py`, ...); what they
share is in `tests/support.py`. Run the tests for what you're changing while you work,
and the whole suite once before `ready`. Reviewers run only the tests that check a
claim, never the whole suite.

The tests use fake `rex` and `gh` programs and a local bare repository as origin, and
run `bin/bullpen` as you do. A test of a function on its own is a Rust test, beside
its module (below). Each test removes its temp folder when it ends.
Each module runs inside a temp folder of its own and fails if anything is left in it
afterwards.

## The Rust tests

`core` and `cli` keep their tests the board's way, below: beside each module, as
`test_<module>.rs`.

## The board's tests

The board's tests sit next to the code they test, one file per module:
`board/src/model/detail.rs` ends with

```rust
#[cfg(test)]
#[path = "test_detail.rs"]
mod tests;
```

and its tests are in `board/src/model/test_detail.rs`. The test file is a child of its
module, so it can use the module's private items. Helpers several test files share are
in the folder's `testing.rs` (`board/src/model/testing.rs`: `task()`,
`Config::default_for_test()`, ...). Add a test to the end of its module's
`test_<module>.rs`. The `test_` name means bullpen treats the file as a test file when
it rebases: two tasks that both added tests at the end of it keep both.

`board/src/app/` is the same: `App` and its modes in `mod.rs` (tests in `test_app.rs`),
the keys in `keys.rs`, the background commands in `jobs.rs` and reloading the board's
program in `restart.rs`. A test builds a board with `App::for_test(tasks)` (in
`board/src/app/testing.rs`, with `press()`, `Scratch` and the other shared helpers),
so a new field on `App` is set in `App::new` alone.

`board/src/ui/` is the same too: `mod.rs` sizes the board once (`Sizes`) and picks
what to draw (tests in `test_ui.rs`); the list in `list.rs`, the message and key rows
in `bottom.rs`, the full-screen views (a peek, a prompt, a question) in `overlay.rs`
and the `?` list, with `BOARD_KEYS`, in `help.rs`. Their tests draw the board into a
test terminal and read it back with `screen()`, `row()` and `tone_of()` (in
`board/src/ui/testing.rs`).
