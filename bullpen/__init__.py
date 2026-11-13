"""bullpen: a coordinator and its worker agents, each in its own Rex pane.

Tasks live outside the repo, in $BULLPEN_STATE (default ~/.local/state/bullpen).
Each started task gets a git worktree, a branch and a Rex pane running a
worker agent. A Lua watcher (lua/watch.lua, run with `rex do --all`) turns the
workers' program status into news lines for the coordinator: typed into its pane,
or logged for it to follow with `bullpen news --follow`.

bin/bullpen runs the Rust build (target/bullpen), which hands the commands Python still
owns to bin/bullpen-py: it finds this package beside it and runs cli.main.
cli.py builds the parser from each module's commands; docs/code.md lists the modules.
"""
