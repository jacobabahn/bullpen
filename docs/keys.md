# Keys

`bullpen keys --install` adds a line to the end of your Rex config (`init.lua`,
or `REX_CONFIG`) and runs `rex config reload`. It only appends, so what's above
the line stays first, and it adds nothing when the line is there (it still
reloads). If the config already loads another bullpen's `keys.lua`, it says so
and changes nothing: edit that line. `bullpen keys` only prints the line. It defines
four actions, also in the command palette under Bullpen, and binds:

- `cmd+;` then `n`: the next task that needs you. Again: the one after it. Once
  nothing needs you, the board, with the task ready to accept (else finished) that
  has waited longest selected.
- `cmd+;` then `b`: the board.
- `cmd+;` then `c`: the coordinator.
- `cmd+;` then `j`: jump to any open task. A picker opens in the middle of the
  window, listing every open task (the repository's name when there is more
  than one, then its mark in the board's colour, ID and title) of every
  repository bullpen knows, this one's first, each with what needs you first,
  then what's ready, then the rest in the board's order. Type part of it: each
  word you type must be in one word of the line, its letters in order but not
  necessarily together (`lgn` finds "login", `rl` finds "Rate-limit", but
  "digest" doesn't find "Migrate sessions table"), and the closest matches come
  first. `↑` `↓` (or `ctrl+p` `ctrl+n`) move, `⏎` goes to that task as
  `bullpen go` does, and `esc` closes the picker. A task without a pane (queued
  or stopped, say) opens its board with that task selected, where its keys
  (`s`, `r`, `i`) are: its coordinator's board, else one that shows other
  coordinators' tasks (`o`). If even that fails, the picker says why after the count
  and stays open. It works from any pane
  (`bullpen pick --pane`; `bullpen pick` runs it in the terminal you're in).

And in a worker's or reviewer's floating pane, `ctrl+escape` puts it away and
returns you to the board (`bullpen hide`): a capsule is minimized again (in the workers
tab too, with `--workers tab`, and you're back in the board's tab), and a
background pane goes back to the background, out of every tab. In a pane with a tab of
its own (`--workers tabs`), and anywhere else, the key goes through. Those panes say
so in their label ("bullpen: BP-3 · ctrl+esc hides"). See [Where panes
live](settings.md#where-panes-live).

`setup{ prefix = "ctrl+b" }` moves them behind another key, and
`keys = { next = "cmd+shift+j", jump = "cmd+k" }` sets single ones. Run `rex config reload` after.

Rex reads `keys.lua` only when its config loads. An accept that changes it says
"Keys changed: run `rex config reload`" once, on the board's message line (in place
of "Accepted") and in the news. `cmd+;` then `b` with no board running (it was
quit, or its coordinator was started with `--no-board`) starts one beside the
coordinator, as `bullpen up` would, and shows it.
