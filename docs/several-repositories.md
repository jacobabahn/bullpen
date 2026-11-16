# Several repositories

Each repository has its own coordinator, board and tasks, but bullpen knows them
all: their state is under `~/.local/state/bullpen/repos/`. A repository whose
checkout is gone is left out of what follows.

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
  repositories every 10 seconds, and reads a repository's tasks again only when
  its tasks folder changed. `bullpen config --board-repos off` turns it off
  (`--global` for every board).
