# Notifications

When a task comes to need you (its worker or reviewer is at a prompt, the
coordinator ran `bullpen ask`, or triage went to you), you get a native
notification, titled `bullpen · <repo>`: "BP-3 needs you: <the question>".
`bullpen config --notify all` also sends one when a task is ready ("BP-3 is ready:
<title>"), finished, or auto-accepted; `--notify off` sends none; `needs` is the
default. Each goes out once, when the task's status changes, and not while the
task's own pane or its coordinator's board has focus in Rex. When several tasks
come to need you at once (within 10 seconds of a notification for another), the
first is sent as usual and the rest wait for a 3-second lull, then go out as one:
"3 tasks need you: BP-3, BP-4, BP-5" (a single one goes out as it was). The
watcher sends it, on the round that types waiting news.

When pickup pauses because self-started tasks wait for you (see
[Pickup](backlog.md#pickup)), you get one: "Pickup paused: BP-140 and BP-141 wait for
you", once a pause, from the first one's pane, unless notify is `off`.

Bullpen writes the notification (OSC 777) to the terminal of the task's pane, or of
its board, or its coordinator, when the task's has none, and Rex for Mac shows it:
clicking it opens that pane's tab. Turn on Settings > Notifications > System
Notifications > Terminal notifications in Rex for Mac. Each one, and why one wasn't
sent, is logged in `notify.log` beside the tasks.
