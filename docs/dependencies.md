# Dependencies

A task can wait on others: `bullpen add "..." --brief-file f --after BP-8`
(repeat `--after`, or give several as `BP-8,BP-9`). `bullpen after BP-9 BP-8`
sets or changes what an existing task waits on, and `bullpen after BP-9 --none`
clears it. Both refuse an unknown task, a dropped one, and a cycle (BP-8 waiting
on BP-9 while BP-9 waits on BP-8).

`start` refuses a task whose dependencies aren't all accepted, naming them;
`--force` starts it anyway. The board, `ls` and `show` say "Waits on BP-8" for it.

When a task is accepted, bullpen starts each queued task whose dependencies are
now all accepted, in ID order, as long as fewer than `max_running` tasks are
working (3 by default; `bullpen config --max-running 5` changes it). A task in
review doesn't take a slot: reviewers are short and cheap. So when a worker
hands its task off (to review, ready, or finished), or an accept or a drop frees
a slot, the next queued task starts too. A task sent back from review works
again and takes a slot again, even if that puts the count over `max_running`:
it is finishing work already started, and the queue starts nothing more until
the count is under the limit. A worker that stops (at a prompt, with a question,
its pane gone) keeps its slot: it comes back to its task. The queue hears of every
status change, whatever made it, and starts the next tasks once the command that
made it has done its own work, so their news follows its news. Only tasks with dependencies are queued like this: a plain todo task waits
for the coordinator (or you) to start it. Each start is one news line, `BP-9
started (BP-8 was accepted)`. While the usage limit is reached nothing starts;
the news says which tasks could.

If a dependency is dropped, the tasks waiting on it stay todo and one news line
asks what to do: `bullpen after BP-9 --none` (or other tasks) and `start` it, or
drop it.

A task with dependencies gets one more section in its brief when it starts, "What
this builds on": each dependency's title, its summary (the PR description its
worker wrote) and where its review findings are, so it knows what just landed.
