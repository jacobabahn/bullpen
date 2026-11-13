# Tests

```sh
python3 tests/run.py                  # the whole suite, across CPUs: about 75 s on 16
python3 tests/run.py tour local_mode  # only tests whose module, class or name has one of these
python3 tests/run.py --slowest 20     # and list the 20 slowest
python3 -m unittest discover -s tests # the same tests one at a time: about 9 minutes
cd board && cargo test                # the board's: well under a second once built
```

There's a module per area (`tests/test_tour.py`, `tests/test_news.py`, ...); what they
share is in `tests/support.py`. Run the tests for what you're changing while you work,
and the whole suite once before `ready`. Reviewers run only the tests that check a
claim, never the whole suite.

The tests use fake `rex` and `gh` programs and a local bare repository as origin, and
run `bin/bullpen` as you do. Tests that call bullpen's functions directly import them
from the `bullpen` package (`tests/test_helpers.py`). Each test removes its temp folder
when it ends.
Each module runs inside a temp folder of its own and fails if anything is left in it
afterwards.

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

`main.rs` and `ui.rs` still keep their tests in a `mod tests` at the bottom, until
they are split the same way.
