# Glossary

Words bullpen's code and docs use for a task's lifecycle. The code is in the
"task lifecycle" section of `bin/bullpen`.

**Task status**: where a task is: `todo`, `working`, `review`, `ready`, `finished`, `triage`, `needs`, `stopped`, `done`, `dropped`. It changes only through `set_status`.

**Stop**: why a task waits on someone, recorded as `{"by": "worker" | "reviewer", "kind": ...}`: at a prompt, failed, ended its turn without `ready`, exited, its pane gone after a Rex restart, a message that didn't reach the worker, or not continued after the usage limit. Only a task in `triage`, `needs` or `stopped` has one. The task's `why` says the same thing in words, for people and the board; code never goes by its wording.
_Avoid_: reason (for the recorded stop), needs (for any stop).

**Triage**: a stop the task's coordinator can see to. It becomes **needs** if no coordinator is running, or once `triage_minutes` pass (escalation).

**Needs**: a task that needs the person: an agent at a prompt, an `ask`, or an escalated triage.

**Ask**: the coordinator puts a question to the person about a task (`bullpen ask`). The question becomes the task's `why`; its stop stays as it was.

**Ok**: the coordinator has seen to a triage stop (`bullpen ok`). The task goes back to working, or to review for a reviewer's stop, and its idle agent isn't a stop again until its next turn.

