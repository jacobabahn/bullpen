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
run `bin/bullpen` from bytecode compiled once: Python would otherwise compile it on
every call, most of a call's time. Each test removes its temp folder when it ends.
Each module runs inside a temp folder of its own and fails if anything is left in it
afterwards.
