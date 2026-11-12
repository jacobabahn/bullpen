# Several coordinators

A repository can have more than one coordinator at a time, each an agent in its
own pane. `bullpen up --here` in another agent's pane adds it as a coordinator and
leaves the others as they are; `bullpen up` from elsewhere starts one only when
none is running. Each coordinator has an ID (`c1`, `c2`, ...) and its own board
beside it. With several coordinators, each board shows only the tasks its
coordinator sees to (see below), plus those nobody sees to. Its top line names its coordinator and says what it
hides ("+4 in c2 · o shows"). `o` shows the other coordinators' tasks too, in
their usual groups, with dimmed titles that start with their coordinator's ID
("c2 · Fix the login"); their closed ones follow `f`. Each board remembers its
choice (its entry's `board_others` in config.json; `bullpen config
--board-others on --coordinator c1` sets it from a shell). `go --next` from a
coordinator's pane, its board or one of its workers skips the tasks its board
hides, as do the board's `n` and selection. With one coordinator, every board
shows every task.

A task belongs to the coordinator that added it: the one whose pane ran `bullpen
add`, or the first coordinator when `add` ran anywhere else (`bullpen show`
says which, with several). One rule says who **sees to** a task: its own
coordinator while that one's pane is running, else every running coordinator,
else nobody, and then the task needs you. Those coordinators get its news (typed
into their panes, or printed by their own `bullpen news`), its stops as triage,
its panes in their tab, and it is on their boards. News that isn't about one task
(several tasks to start after an accept, a dropped dependency, the usage limit
resetting, rebases after an accept) goes to every coordinator, as does news that
was waiting to be typed into a coordinator whose pane is gone. `bullpen adopt BP-3`, run in a coordinator's pane, makes the
task that coordinator's own (its panes move to that coordinator's tab too).
Accepting or dropping a task from a coordinator's pane tells its own
coordinator if that is another one.

When a coordinator's pane closes, bullpen forgets that coordinator and closes its
board a few seconds later, if Rex still runs its session (so a Rex restart or
shutdown forgets nobody). `bullpen up` does the same for any it finds gone.
After a Rex restart it finds the coordinators it started by their labels; one
made with `--here` isn't found again: run `bullpen up --here` there again, then
`bullpen adopt` its tasks. `max_running` and the queue are shared by all of
them. Worker panes go in their own coordinator's tab. `bullpen go` from a
board goes to its coordinator, from a worker to the task's coordinator, and
from elsewhere to the first one.

A config from before this, with one coordinator, loads as coordinator `c1`, and
its tasks (which name no coordinator) are news for every coordinator until one
adopts them.
