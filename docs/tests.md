# Tests

```sh
python3 tests/run.py                  # builds the command line and the fakes, then the whole suite, across CPUs: about 12 s on 16
python3 tests/run.py -j 8             # on 8 processes instead
python3 tests/run.py tour local_mode  # only tests whose module, class or name has one of these
python3 tests/run.py --slowest 20     # and list the 20 slowest
python3 -m unittest discover -s tests # the same tests one at a time: about 2 minutes (build first)
bin/bullpen-cargo test --workspace    # the Rust tests: core's, the command line's and the board's
```

The tests run `bin/bullpen`, which runs the Rust build (`target/bullpen`), and the fake
`rex` and `gh` (below). So `tests/run.py` builds the command line and the fakes first, at
low priority, when their source changed since the last build (it writes the build's
stamps, `target/bullpen.json` as `bullpen build` does and `target/fakes.json`), and
stops with cargo's error if they don't build. A test module run any other way
says so when a build is missing or older than its source: run `tests/run.py` (for the
command line, `bin/bullpen build` does too). The board isn't built for the tests: they give bullpen a fake one
(`BULLPEN_BOARD`). Nor is this checkout, by anything a test runs: `tests/support.py`
sets `BULLPEN_NO_BUILD=1`, so bullpen never builds, or takes the build lock in, the
checkout it runs from (docs/rebuilds.md). The tests about building build a copy of
bullpen, with a fake cargo (`tests/test_board_build.py`).

The test processes run at low priority (nice 10), as does everything they start, so
several agents can run the suite at once and the machine stays responsive. They use
every CPU, and give way when anything else wants one.

There's a module per area (`tests/test_tour.py`, `tests/test_news.py`, ...); what they
share is in `tests/support.py`. Every test built on it is also held to one rule about
news: each line bullpen logged during the test has to say what the coordinator does
(a command, or who to tell) or match a row of the table in the coordinator's guide
(`check_news`, with the rows in `NEWS_ROWS`; see [News](news.md)). A new news line
that does neither fails the test that sent it: give it its instruction, or a row. Run the tests for what you're changing while you work,
and the full suite once, right before `ready`: `python3 tests/run.py` plus
`bin/bullpen-cargo test --workspace` (plain `cargo` isn't on an agent's PATH; see
docs/rebuilds.md). Reviewers run only the tests that check a claim, never a whole suite.

The tests use fake `rex` and `gh` programs and a local bare repository as origin, and
run `bin/bullpen` as you do. The fakes are Rust, in `tests/fakes/` (the crate
`bullpen-fakes`, programs `fake-rex` and `fake-gh`), because bullpen runs them about
10,000 times a suite and a Python one costs about 23 ms a call
(docs/experiments/suite-time.md). Only `tests/run.py` builds them: `install.sh` and
`bullpen build` don't. Each test's `fake/` folder links to them (`target/fake-rex`,
`target/fake-gh`). They answer from files a test drops in that folder (`FAKE_DIR`): the
fake Rex keeps its blocks, layers and windows in `reg.json` and logs each call to
`rex.log`, and a file such as `send_fails_<block>`, `server_down` or `screen` changes
what it does; the fake gh logs to `gh.log` and reads `pr.json`, `gh_signed_out`, `gh_scopes` (the
token scopes `auth status` names),
`merge_refused` (a refused merge, saying the file's text), `meanwhile` (a command
`pr view` runs before it answers: a slow gh) and `issues.json` (the repository's issues,
for `issue list` and `issue view`). The top of `tests/fakes/src/rex.rs` and
`gh.rs` lists them all. They write JSON as Python's `json.dumps` does, since the tests
read their files as text too. A test of a function on its own is a Rust test, beside
its module (below). Each test removes its temp folder when it ends.
Each module runs inside a temp folder of its own and fails if anything is left in it
afterwards.

## The tables

The command table (`cli/src/commands.rs`) and the settings table (`SETTINGS` in
`core/src/settings.rs`) have no copy in the tests. A test reads them from the program
(`bullpen _commands`, `bullpen config --json`, `config`'s groups) and checks facts about
them: no name twice, every command in the help has a help line, a known one is there.
So adding a command or a setting edits no shared test. `tests/test_docs.py` checks
that every command is named in `docs/commands.md` (as `bullpen <name>`) and every
setting in `docs/settings.md` (as its name or its flag, in code), so a new one needs its
line there. Internal ones are exempt by a rule, not a list: a command in no group of the
help, a setting `bullpen config` doesn't list.

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
`Config::default_for_test()`, ...). A test task is built from JSON, as bullpen writes
it: `task("BP-1", "ready").with(json!({"pr": "..."}))`. Add a test to the end of its module's
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
