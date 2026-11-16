# Where things live

- Tasks, briefs, settings and your own rules: `~/.local/state/bullpen/repos/<repo>/` (`BULLPEN_STATE` moves it).
  A change to a task's brief (`bullpen brief`) is kept in `briefs/` there as `<ID>.brief-<n>.md`.
- Settings for every repository: `~/.config/bullpen/config.json` (`XDG_CONFIG_HOME` moves it). See [Settings](settings.md).
  Setup and teardown logs are in `briefs/` there, as `<ID>.setup.log` and `<ID>.teardown.log`,
  and tours with their drafts and logs as `<ID>.tour.json`, `<ID>.tour-comments.json` and
  `<ID>.tour.log` (see [Guided review tours](tours.md)).
- Worktrees: `<repo>-bullpen/<ID>` next to the repository.
- Each task's timeline, for `bullpen stats`: `timeline/<ID>.jsonl` beside the tasks.
  See [Stats](stats.md).
- News: `news.log` beside the tasks (each line names the coordinators it is for,
  as `[c1]`), with `news-<id>.cursor` (how far each coordinator's `bullpen news`
  has read), `outbox-<id>.txt` (typed news waiting for an empty input) and
  `news-<id>.wait` (a pause, the waiter that owns the wait, the last wake). With
  `config --news wake`, the hook that parks the waiter is in the repo's own
  `.claude/settings.local.json` (and that file in its `.git/info/exclude`). See [News](news.md).
- The coordinator guides running coordinators were given, for the diff when it changes:
  `~/.local/state/bullpen/guides/<hash>.md`. See [News](news.md).
- The backlog: `backlog.json` beside the tasks (with `backlog.lock`), its items' briefs
  in `briefs/` as `L<n>.md`. Pickup's `pickup.json` (with `pickup.lock`) beside them, and
  `~/.local/state/bullpen/pickup` while the watcher should look. See [The backlog](backlog.md).
- The GitHub issues read so far, and what was decided on each: `issues.json` beside the
  backlog (with `issues.lock`); the timer's `issues-timer.json` (with `issues-timer.lock`)
  beside it, and `~/.local/state/bullpen/issues` while some repository has `issues` on:
  when the watcher's next read is due. See [GitHub issues](issues.md).
- Overlaps told so far (two open tasks that change the same file): `overlaps.json`
  beside the tasks. See [Accept](accept.md#overlap-warning).
- Notifications: `notify.log` beside the tasks. See [Notifications](notifications.md).
- What each worker and reviewer is doing, for the board: `~/.local/state/bullpen/doing/<pane>.json`,
  written by the watcher (at most every 3 seconds a pane, no news) and removed with the pane.
