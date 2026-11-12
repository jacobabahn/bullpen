# PRD: Guided review tours

Status: agreed 2026-11-12. Option 1 of three considered; options 2 (a live guide agent) and 3 (a tour plus an agent you can ask) are deferred.

## Problem
When a task is ready, the person reviews it by reading the raw diff (`v`) and the
worker's summary. A larger change has no reading order, nothing says why each part
changed, and the only way to ask for changes is a free-form `bullpen tell`.

## Goal
On request, a cheap agent writes a **tour** of a task's change: the change in reading
order, one step per logical change, with a note on why it changed and what to look at.
The person steps through it in a large pane, leaves comments on steps, then accepts or
sends the comments back to the worker as a fix request.

## Decisions

### Asking for a tour
1. **On request only.** No tour is written unless the person asks for one.
2. **`g` on the board** (or `bullpen tour <ID>`). With no current tour, it starts the tour
   agent. The task shows a spinner, and the tour opens when it's written. With a current
   tour, `g` opens it.
3. **Which tasks:** ready and in review. Not working tasks, whose code is still changing.

### The tour agent
4. **A separate, cheap agent**, not the worker or the reviewer.
5. **Headless and paneless** (`claude -p`). It writes the tour file and exits. It does
   **not** count toward `max_running`.
6. **Model:** the `tour_model` setting, Sonnet by default, settable per repo or with
   `--global`.
7. **Inputs:** the task's diff against its base, its brief, the worker's summary, and the
   review findings files.
8. **Read-only tools:** git and file reads in the task's worktree. It writes nothing but
   the tour file.
9. **Failure** (an agent error, or a file that doesn't parse): a line on the task on the
   board ("tour failed: <reason>") and a log. `g` retries. No triage, no news.

### The tour file
10. **Structured** (JSON): an ordered list of steps. Each step has a `title`, a `note`, and
    `hunks` as file plus line ranges.
11. **One step per logical change**, in reading order. A step can span files.
12. **Code is shown from git, never from the agent's text.** The stepper renders the
    referenced hunks from the real diff.
13. **Nothing is skipped:** any hunk no step covers goes into a final step, "Other changes".
14. **Tied to a commit.** A tour stays current while the patch is the same (the same commit,
    or the same patch-id after a clean rebase, as review approvals carry over). Otherwise it
    is **stale**: `g` says the code changed since, and offers a new tour or the old one anyway.

### Reading a tour
15. **A large floating pane**, centred, the way the fuzzy jump picker opens. The step's note
    is at the top and its hunks below, in delta when installed. It closes back to the board.
16. **Optional.** Accepting never requires a tour. The board may mark a ready task as
    toured or not, as a nudge only.
17. **Comments are per step:** `c` opens a text box for the current step. To point at a
    line, the person mentions it in the text.
18. **Drafts persist** per task, so closing the tour keeps them, and opening it again
    resumes where the person was.
19. **At the end, a summary** lists the comments. `s` sends them back. `a` accepts, with a
    warning if comments are unsent. `esc` closes.

### Sending comments back
20. **A findings-style file** with each comment tied to its step, files and lines. The worker
    is told to read it and address each comment, and the task goes back to working.
21. **The coordinator gets a news line**, for information only ("BP-3 sent back by the
    person after the tour: 2 comments").
22. **Re-review:** when the worker runs `ready` again, a follow-up review checks that the
    person's comments were addressed. This round does **not** count toward the review
    round limit.

## Out of scope (later)
- Asking the tour questions (a live agent: option 3).
- Comments on single lines.
- Tours for working tasks.

## Delivery
- **Part 1:** the tour agent, the tour file format, `bullpen tour`, staleness, comment
  storage and sending comments back (with the follow-up review). This includes adding this
  PRD at `docs/prd/guided-review.md`.
- **Part 2** (after part 1): the floating stepper, plus `g` and `c` on the board, the
  "toured" mark, and the end-of-tour summary.
