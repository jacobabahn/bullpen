# Several repositories

Each repository has its own coordinator, board and tasks, but bullpen knows them
all: their state is under `~/.local/state/bullpen/repos/`. A repository whose
checkout is gone is left out of what follows.

- `bullpen repos` says, a line per repository, whether its bullpen is up and what
  waits: its coordinators (each working, idle, blocked on a question in its pane,
  or gone: its pane closed) and what needs you, what's ready and what's running
  (working or in review), else how many tasks are open:

  ```
  bullpen     c3 working · 1 needs you · 2 ready · 4 running
  jj-evolved  c1 gone · 1 ready
  shop        no coordinator · nothing open
  ```

  `--json` prints the whole snapshot it's made from: `{"repos": [...]}`, each with
  its `name`, `root`, state folder (`dir`), `coordinators` (`id`, `block`,
  `session`, `alive`, `word`, and the watcher's last `state`, `text` and `at` for
  its pane), `counts`, its open `tasks` in the board's order (each `{"mark",
  "words", "doing", "task"}`: the board's sentence, what its agent is doing, and
  the task record), task files that don't parse (`broken`) and its `settings`. It
  works from any folder, reads only files (about 10 ms for 160 task files) and asks
  Rex once per coordinator, all at once.
- `bullpen ls --all-repos` lists the open tasks of every repository, under each
  one's name and path, leaving out those with nothing open. It works from any
  folder. `--all` adds the closed tasks; `--json` prints a list of
  `{"repo": ..., "tasks": [...]}`.
- `bullpen go --next --all-repos` goes to the next task that needs you in any
  repository, the one waiting longest first, as `go --next` does in one. In the
  repository it runs for, it skips what that board hides, as `go --next` does.
- `bullpen news --recent --all-repos` prints the last news lines of every
  repository, in time order, each with its repository and time (see
  [News](news.md)).
- `cmd+;` then `m` (`bullpen mission`) is [mission control](mission.md): every
  repository's inbox and tasks on one screen, with the board's keys on each task.
- `cmd+;` then `j` (`bullpen pick`) picks any open task of any repository by
  typing part of it (see [Keys](keys.md)); `bullpen go BP-1 --repo PATH` goes to a
  task of another repository.
- `bullpen say --repo PATH TEXT` types TEXT into that repository's coordinator's
  pane, as if you typed it there, from any pane or folder (without `--repo`: the
  pane's repository, else the folder's). With several coordinators, name one with
  `--coordinator c2`; without it `say` lists them and types nothing. It waits for
  an empty input as typed news does: while something is half-typed there it says
  so, and the watcher types it once the input is empty. It's typed whatever the
  repository's `news` setting, since it's you talking, not news. It says so when
  the coordinator's pane is gone (`bullpen up` there starts one) or refuses typing.
- The board's top line, after the repository's name, names the other
  repositories with something for you: `shop: 1 needs you, 2 ready` (need you:
  needs or stopped; ready: ready to accept, or finished), shortened to
  `shop ●1 ✔2` when the words don't fit, cut (to no less than 8 columns) beside
  this repository's counts, and left out when even that has no room.
  It is amber while one of them needs you. The board looks at the other
  repositories every 10 seconds, through the same snapshot as `bullpen repos`, and
  reads a task file again only when it changed. `bullpen config --board-repos off` turns it off
  (`--global` for every board).
