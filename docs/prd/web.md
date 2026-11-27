# PRD: bullpen web

Status: proposed 2026-11-26 (BP-233); the person's four decisions answered 2026-11-27,
each with the recommended answer (see [Yours to decide](#yours-to-decide)). The rest are
the design's, open to argument. Clickable mockups are in the
state folder's `briefs/design-web/` (`index.html` links every screen), not in the repo.

## Problem
Working with bullpen means Rex panes: a coordinator's, a board, and a worker's or
reviewer's when one needs you. That works at the Mac, in a terminal, with keys. It
doesn't work from a phone, and even at the Mac a worker's pane is a terminal to read,
not a conversation: tool output scrolls past what the agent said, and a question is a
box of text to find and answer with the right key.

## Goal
A page, `bullpen web`, that shows every coordinator and task bullpen runs, across
repositories, as a minimal chat app, on a Mac and on an iPhone in Safari, over
Tailscale. Each agent's conversation reads as chat: what you and bullpen said, what it
said, its tool calls folded into one quiet line, and a card to answer when it asks. The
Rex pane's screen is one tap away (`>_`) when the chat isn't enough. Replies, answers,
accept, drop, diff and tour work from the page, through the `bullpen` commands the
board runs. It updates by itself.

## What v1 is, and isn't

**v1 is:**
- the sidebar: every repository's coordinators and open tasks in three groups (Needs
  you, Active, Done), with the board's marks, colours and words, and ⌘K to jump;
- a chat for each coordinator, worker and reviewer, from its Claude Code transcript;
- prompt cards: a permission prompt (from the pane's screen), an AskUserQuestion
  (from the transcript), and a coordinator's `bullpen ask` question;
- typing to a worker (`tell`) or a coordinator (`say`), and the header's actions:
  accept, drop, diff, tour (read), start, resume;
- `>_`: the pane's screen, live, in its colours;
- pushed updates, a phone layout, light and dark;
- reachable only through `tailscale serve`, for the people `web_users` names.

**v1 isn't:**
- a way to start tasks or write briefs: you ask the coordinator, in its chat;
- tour comments (the board's stepper keeps them), the backlog, settings, stats;
- keys other than a prompt's options (no esc, arrows or ctrl keys into a pane);
- images in the chat, files from `tool-results/`, or subagents' transcripts;
- notifications (Web Push) or an installable app;
- anything for an agent that isn't Claude Code: it gets the `>_` view and no chat;
- T3 Code, or a replacement for the board or mission control in Rex.

## Decisions

### Beside T3 and mission control
1. **Build `bullpen web` over the Rex workers; set the T3 host aside.** T3 would give
   a chat for free, but its sidebar can't group, colour or show long titles (no
   nesting, no badges, titles cut at about 40 characters), and it can only show
   agents that run in T3. The web page shows the agents bullpen already runs, with the
   board's groups and words. BP-224's `Host` trait stays: it's how the commands the page
   calls reach an agent, and a T3 host could still be added under it later. **Yours:
   yes, the T3 host is set aside.**
2. **The page is mission control's rework.** Its sidebar is mission control's inbox
   and repositories, and the panels mission control still lacks (the coordinators,
   news) are the page's coordinators and their chats. Mission control in Rex stays as
   it is, kept working, with no new panels. **Yours: yes.**

### Shape
3. **A new crate, `web/` (`bullpen-web`), run by `bullpen web`**, as `bullpen board`
   runs `bullpen-board`: built by `cli/src/build.rs` into `target/bullpen-web`, so
   `cli` keeps its few crates. It links `bullpen-core` for the snapshot and the
   transcript reader, and runs the `bullpen` program for every action.
4. **axum on tokio**, for its routes, server-sent events and middleware. It adds
   `axum`, `tokio` (`rt-multi-thread`, `net`, `fs`, `sync`, `time`, `macros`),
   `tokio-stream` and `notify` (the board's already). A blocking server (`tiny_http`, a
   thread per connection) would add less, but its long-lived event streams need
   hand-made plumbing, and the crate list needs the person's yes either way. **Yours:
   yes, these four.**
5. **Server-sent events for pushes, POST for actions.** Updates only go one way; an
   action is a request with an answer. No WebSocket.
6. **One static page with no build step**: `index.html`, `app.js` and `app.css`,
   plain JavaScript, compiled into the program with `include_str!`. No framework, no
   bundler, nothing fetched from the internet, so the repo stays Rust and Python.
7. **The page doesn't parse transcripts.** The server turns records into chat items
   and sends those, never a raw record (records carry system prompts, account IDs and
   e-mail addresses). See [Transcripts into chat](#transcripts-into-chat).
8. **Where it runs:** `bullpen web` in the foreground, in any pane or shell. It reloads
   itself when its program is rebuilt, as the board does. Keeping it running across
   restarts (a user service, or a Rex session as mission control has) is for later.

### Modules
In `core/src/chat/` (no new crates; the board could show a chat's last line later):
- `mod.rs`: finding an agent's transcript (below) and reading it from a byte offset.
- `records.rs`: one JSONL record into what it says, or nothing.
- `items.rs`: records into chat items: merging an API message's records, pairing a
  tool call with its result, grouping a run of calls into one line.

In `web/src/`:
- `main.rs`: the command line, the socket, reloading on a rebuild.
- `auth.rs`: who may connect, the host and same-origin checks, the response headers.
- `routes.rs`: the routes, each a few lines over the modules below.
- `fleet.rs`: the snapshot (`bullpen_core::fleet`), read again when the state folder
  changes, with the coordinators' liveness from `bullpen repos --json` every 5 seconds.
- `events.rs`: the event stream: what each client watches, and what to send it.
- `watch.rs`: `notify` on the state folder and on the transcripts being watched.
- `act.rs`: running a `bullpen` command for an action: in the task's repository, with
  `REX_BLOCK` unset (so a `tell` is the person's, not `Coordinator: `), one at a time
  per task, its first line of output or its error back.
- `markdown.rs`: what an agent said, from Markdown into escaped HTML.
- `screen.rs`: a screen's lines with SGR codes into escaped HTML spans.
- `page/`: the page's three files.

### Routes
Every route needs the checks in [Security](#security); without them the answer is a
bare 403. A task or coordinator is named by its repository's name and its ID
(`bullpen/BP-231`, `bullpen/c4`); `who` is `worker` (the default) or `reviewer`.

| Route | Returns |
| --- | --- |
| `GET /` | the page (and `/app.js`, `/app.css`) |
| `GET /api/fleet` | the snapshot: `bullpen repos --json`'s shape, plus each task's group (`needs`, `active`, `done`) and the closed tasks of the last two days |
| `GET /api/chat/{repo}/{id}?who=&before=` | `{agent, source, items, cursor, more}`: the last 200 items (`before`: the 200 before an item), the transcript's `source` (`recorded`, `title` or `none`), and `cursor`, the byte offset read to |
| `GET /api/prompt/{repo}/{id}` | `bullpen prompt` (its JSON, with W2's `draft`): `{who, question, options, id, screen, draft}`; `who` is whichever agent is at the prompt (`coordinator` for a coordinator's, with `--coordinator`) |
| `GET /api/screen/{repo}/{id}?who=` | `{lines, at}`: the end of the pane's screen as HTML spans |
| `GET /api/diff/{repo}/{id}` | `bullpen diff ID` as `{files: [{path, added, removed, hunks}]}` |
| `GET /api/tour/{repo}/{id}[?step=N]` | `bullpen tour ID --show --json`, or `--step N --json` |
| `GET /events?watch={repo}/{id}[/{who}]` | the event stream (below) |
| `POST /api/{repo}/{id}/tell` `{text}` | `bullpen tell` (a worker); for a coordinator, `bullpen say --coordinator` |
| `POST /api/{repo}/{id}/answer` `{key, prompt}` | `bullpen answer ID KEY --prompt PID` |
| `POST /api/{repo}/{id}/reply` `{text}` | `bullpen reply` (the coordinator asked) |
| `POST /api/{repo}/{id}/{accept,drop,start,resume,tour}` | that command; `drop` takes `{force}` |

A POST answers `{ok, said, error}`: the command's first line, or its error. Accept,
drop, start, resume and writing a tour take seconds, so they answer at once with
`{ok, running: true}`; the task's mark spins (from the event stream) until its file
changes, and the line it said comes as a `done` event.

### Events
One stream per page, `EventSource('/events?watch=…')`; switching chats opens a new one.
Each event has an `id`, so a stream that drops (a phone waking) resumes with
`Last-Event-ID` and gets what it missed, or a fresh `fleet` when that's too old.

| Event | When | Data |
| --- | --- | --- |
| `fleet` | on connecting, then whenever the snapshot changes | the whole snapshot (a few tens of KB; sent only when its hash changes) |
| `chat` | the watched transcript grew | `{items, cursor}`: the new items, and items changed (a tool line whose call finished) by their `id` |
| `prompt` | the watched agent is blocked, every second while it is | `{question, options, id, screen}`, or `null` when the prompt is gone |
| `screen` | `>_` is open, every second while it changes | `{lines, at}` |
| `done` | an action ran in the background finished | `{task, ok, said, error}` |
| `ping` | every 15 seconds | nothing; keeps proxies and Safari from closing the stream |

### From a change on disk to the page
- **The snapshot.** As mission control: a recursive `notify` watch on the state folder,
  a read of the snapshot 100 ms after the last change (`fleet::Reader` reads a file
  again only when its time or size changed, about 10 ms for 160 task files), and once
  a second in case a change was missed. A task's status reaches the page in about
  200 ms. What an agent is doing comes from `doing/<pane>.json`, which the watcher
  writes at most every 3 seconds a pane: that's as fresh as the board's. Liveness of
  coordinators is Rex's to say: `bullpen repos --json` every 5 seconds, in the
  background, as mission control does.
- **A chat.** A `notify` watch on the transcripts that someone is watching, and only
  those. On a change, the server reads from its byte offset to the last whole line
  (a line still being written waits for the next change), turns the records into
  items, and sends them. Claude Code writes each record when it happens, so a tool
  call shows within about 100 ms.
- **A prompt and the screen.** bullpen hears nothing from Rex when a screen changes,
  so these are polled, as the board's `i` and `p` poll: `bullpen prompt` once a second while a
  watched agent is blocked, and the screen once a second while `>_` is open. Nothing
  is polled for a chat nobody has open.
- **The watcher stays as it is.** `lua/watch.lua` already writes the files the server
  reads; it needs no hook for the page.

### Pane to transcript
What bullpen does today (checked in the code and in `~/.claude/projects/`):
- A worker starts as `claude --permission-mode auto -n "BP-3: <title>" <first message>`
  in its worktree (`cli/src/start/launch.rs`); `resume`, and a hand-back when its pane
  is gone, add `--continue`. A reviewer starts in **the same worktree** with
  `-n "BP-3 review"` (`cli/src/review.rs`). A coordinator starts in the repository's
  checkout with `-n "bullpen: <repo>"` (`cli/src/up.rs`). Nothing passes or records a
  session ID.
- So a worktree's project folder (`~/.claude/projects/` and the worktree's path with
  `/` and `.` as `-`) holds the worker's and the reviewer's transcripts side by side,
  `<session id>.jsonl` each: BP-232's has three (the worker, two review rounds). The
  newest `.jsonl` isn't reliably the worker's. And `--continue` takes the newest
  conversation in the folder, which after a review is the reviewer's (in the backlog
  as L26).
- The worker's Stop hook (`_worker_stop`, `cli/src/stop_hook/turn.rs`) is given
  `session_id` and `transcript_path`, keeps the session ID in `turns/BP-3.json` and
  drops the path. The hook is in the worktree's `.claude/settings.local.json`, so the
  reviewer, started in the same worktree, runs it too: a reviewer's turn that ends
  writes its own session ID over the worker's record. A coordinator's hook
  (`news --wake`, with `config --news wake` only) reads the session ID for its waiter.
- Each transcript starts with a `custom-title` record holding the `-n` name, written
  when the agent starts, before its first turn ends.

The design:
9. **bullpen chooses the session ID.** When the agent is Claude Code (`is_claude`),
   `launch`, the reviewer's start and `up` pass `--session-id <a new UUID>`, and record
   it: the task's `chat` (the worker's), its review's `chat` (the reviewer's), a
   coordinator's `chat` in its entry. The page finds the file as
   `~/.claude/projects/*/<id>.jsonl`, with no need to rebuild the folder's name.
   Resuming passes `--resume <id>` instead of `--continue`, which fixes L26.
10. **The Stop hook corrects it, for its own agent only.** `_worker_stop` also keeps
    `transcript_path`, after telling whose turn ended, since the worker and the
    reviewer share the hook: a session ID that is the task's `chat` is the worker's,
    one that is its review's `chat` is the reviewer's, and an unknown one (a `/clear`
    starts a new conversation) is told by the transcript's `custom-title` (`BP-3: …`
    the worker, `BP-3 review` the reviewer), else ignored; one with no title to read
    (no transcript) writes the turn record as before and moves no `chat`. The worker's turn goes in
    `turns/BP-3.json` as now; the reviewer's moves its review's `chat` and never touches
    the worker's record. The page follows a recorded `chat` when the hook moves it.
    The coordinator's waiter keeps the path too.
11. **Agents started before this ship** are matched by title: the newest `.jsonl` in
    the agent's folder whose `custom-title` is its `-n` name. The chat says it was
    matched by title (`source: "title"`), in small type under its first message.
12. **A compacted conversation stays one file** (a `compact_boundary` record, then the
    summary and the rest): the chat shows "Conversation compacted" there and keeps what
    came before above it.

### Transcripts into chat
A transcript is JSONL: one record a line. Most are bookkeeping. BP-232's worker
transcript (2,784 lines, 20 MB) has 518 `assistant` and 291 `user` records; the rest
are `data-flow-records`, `api-request`, `attachment`, `api-request-blob` and the
per-launch title, mode and owner records. The reader keeps a whitelist:

| Record | In the chat |
| --- | --- |
| `user`, text, the first | the brief: "Started from its brief", naming `BP-3.full.md` (or the review brief) |
| `user`, text, starting `bullpen · ` | a news line, apart from your messages, its task a link |
| `user`, text, starting `Coordinator: ` | the coordinator's bubble, labelled with its ID |
| `user`, text, other | your bubble; a `<pasted_content>` in it folds to "Pasted text, N lines" |
| `user`, text with `isMeta` | hidden (image notes and the like) |
| `user`, `<task-notification>` | hidden (a background job finishing; the tool line shows it) |
| `user`, `tool_result` | not an item: it completes its call (done, or the error's first lines) |
| `assistant`, `text` | the agent's words, as Markdown (below) |
| `assistant`, `thinking` | hidden (empty in practice: the text is withheld, only a signature kept) |
| `assistant`, `tool_use` | a tool call, in the tool line of its run (below) |
| `assistant`, `tool_use` AskUserQuestion | a question card (see [Prompt cards](#prompt-cards)) |
| `system`, `turn_duration` | a divider: "Worked for 1 h 29 min" |
| `system`, `compact_boundary` | a divider: "Conversation compacted" |
| `system`, level `error` | a red line with its text |
| `system`, other (`stop_hook_summary`, `away_summary`, `informational`) | hidden |
| `queue-operation`, `enqueue` | a faint bubble, "waiting for its turn", until its `user` record comes |
| `attachment`, `api-request*`, `data-flow-records`, `file-history-*`, `cost-state`, `tools-used`, `derived-from`, `custom-title`, `agent-name`, `mode`, `permission-mode`, `session-owner`, `atis-latch`, `last-prompt`, anything new | hidden; the title and permission mode feed the header |
| `isSidechain: true` | hidden (subagents write files of their own) |

- **An API message is several records**, one content block each, sharing
  `message.id`: they're merged first.
- **A run of tool calls is one line.** The calls between two things the agent said are
  one item: "Ran 5 commands, read 2 files", with the time from the first call to the
  last result. Open, it lists each call: Bash's `description` when it has one (workers
  often don't), else the command's first line; a file's path from the worktree for
  Read, Write and Edit; an agent's description for Agent; then done, or the error and
  the end of its output (from `toolUseResult`: `stdout`, `stderr`). A call still
  running shows as the line's own words with a turning mark: "Running python3
  tests/run.py test_queue…".
- **Markdown, kept small, made into HTML by the server** (`web/src/markdown.rs`):
  paragraphs, emphasis, inline and fenced code, lists, tables, links (`http` and
  `https` only). Everything is escaped first; no HTML from a transcript reaches the
  page, no image is fetched, and the page has no Markdown code of its own.
- **Long chats**: the first read of a file parses it whole (once per file while the
  server runs; how long a 20 MB one takes is W3's to measure), keeps its items and its
  offset, and sends the last 200; "Earlier" pages back.

### Prompt cards
The one loud thing on a screen: an amber-edged card at the end of the chat, with
options that are buttons large enough for a thumb, each with its key.

13. **A permission prompt, from the screen.** Permission prompts aren't in the
    transcript. While a watched agent is blocked, the server reads `bullpen prompt`
    (the question, its options and their keys, the prompt's ID, the screen) every
    second, and the card shows the question, what's asked (the box's lines above it:
    "Bash command", the command, its description) and the options. A press posts
    `answer` with the prompt's ID: bullpen reads the screen again and refuses if the
    prompt changed, and the card says so and shows the new one. "No, and tell Claude
    what to do differently" is pressed like any option; the composer then says "Tell
    it what to do instead".
14. **AskUserQuestion, from the transcript.** A `tool_use` named AskUserQuestion with
    no result yet is a card with each question (its `header` as a tab), its options
    and their descriptions, from the input
    (`{questions: [{question, header, multiSelect, options: [{label, description}]}]}`).
    It's answered on the screen, where the same question is a menu: for each question,
    `answer` presses its option's number, checking the screen shows that question.
    When the `tool_result` comes, the card shows what was chosen (`toolUseResult.answers`).
    v1 answers single-choice questions; a multiple-choice one, "Type something" and
    the last "Submit" step of several questions need the TUI's keys checked first
    (task W0), and until then the card offers `>_`.
15. **A coordinator's question** (`bullpen ask`): the task's card has the question and
    a line to answer on; it posts `reply`, as the board's `i` does.
16. **A coordinator's own prompt** (its pane at a permission prompt) is a card in its
    chat. `prompt`, `answer` and `peek` only take tasks today; W2 adds `--coordinator`.

### Typing to an agent
17. **A worker: `bullpen tell`; a coordinator: `bullpen say`.** In a reviewer's chat
    the composer still writes to the worker, as the board's `t` does. Text sent shows
    as a faint bubble until its `user` record (or Claude Code's queue record) arrives.
18. **Never into a half-typed input.** `say` already waits for an empty input (the
    outbox). `tell` doesn't look, and `prompt` (with its new `draft`) refuses a task
    that isn't stopped at a prompt, so W7 has `tell` look first (the host's
    `has_draft`, as `say` does): with something half-typed in the pane, the page says
    so and sends nothing.
19. **One line.** A line break in what you type is sent as a space in v1 (as a paste on
    the board is), since typing a line break into the pane would send it early.

### The header's actions
20. **The task's keys, as buttons, through the same commands.** A ready or finished
    task: its risk chip, the size of its change, where it stands, then Diff, Tour, Drop
    and Accept. A queued task: Start. A stopped one: Resume. Accept and drop ask first,
    in the board's words (`board/src/model/confirm.rs`), risky ones in red; a drop the
    worktree refuses offers "Drop anyway" (`--force`).
21. **Diff and tour are read in a drawer** beside the chat (the whole screen on a
    phone): `bullpen diff`'s files, then their hunks; a tour's steps with their notes
    and hunks from `--step`. With no tour, Tour starts writing one and opens it when
    the task's file says it's written. Commenting stays on the board.
22. **Open in Rex** (`bullpen go --switch`) on the Mac only: it moves the Rex app.

### `>_`: the pane's screen
23. **The end of the screen, in its colours**, from `bullpen peek --vt` (the coloured
    lines the board's `p` shows), as the chat's other view, polled each second while
    open. `screen.rs` turns SGR codes into spans: the 16 colours as classes the page
    themes, 256 and true colours as inline colours, bold, dim, italic, underline,
    reverse. Never Rex's `--format html` passed through: the text is escaped, so
    nothing a pane prints can run in the page.
24. **Its keys are the prompt's options** (through `answer`), and its line is `tell`'s.
    No arbitrary keys in v1.

### Phone
25. **The sidebar is a screen of its own**; a chat has a back button, its actions move
    to a row under the header, the card's options grow to 50 points, inputs are 16
    points (so Safari doesn't zoom), and the composer sits above the home indicator.
    `>_` scrolls sideways rather than wrap a terminal's lines.

### The sidebar
26. **Three groups, across repositories**, each row naming its repository:
    - **Needs you**: what needs you or stopped, then what's ready or finished, then any
      coordinator blocked at a prompt; the one waiting longest first, as `go --next
      --all-repos` takes them. A task that needs you sits on the amber wash with its
      bar; an age over 5 minutes turns into a filled pill.
    - **Active**: each repository's coordinators (working, idle, gone), then its
      tasks in review, working, the coordinator's, queued.
    - **Done**: tasks closed in the last two days, dim.
27. **⌘K** finds any of them by ID, title or repository; `n` first, the next that needs
    you. The page's title counts what needs you, so the tab says it.

### Security
The page types into agents running in auto mode: whoever can use it has a shell as the
person. The rule: it must give nobody a power they don't already have.
28. **No TCP port at all.** `bullpen web` listens on a Unix socket,
    `<state>/web.sock`, made `0600` in a `0700` folder. Only the person (and root) can
    connect, and the person can already run `bullpen` there. Nothing listens on the
    LAN, the Tailscale address or localhost.
29. **Reached through `tailscale serve`**, which proxies to a socket:
    `tailscale serve --bg unix:<state>/web.sock`. It serves HTTPS on the machine's
    tailnet name, only to the tailnet, and adds `Tailscale-User-Login` to each request,
    replacing any the client sent. `bullpen web` prints this line; it never runs it.
    Never `tailscale funnel`.
30. **Who.** A request's `Tailscale-User-Login` must be in `web_users` (a global
    setting). Unset, it is the login of the Tailscale user this machine is logged in
    as (`tailscale status --json`), so it works for the person alone with nothing to
    set. A tagged device or a shared-in user has another login, or none, and is refused.
    **Yours: yes, the machine's own login by default, `web_users` the override.**
31. **Where.** The `Host` header must be the machine's tailnet name (`web_host`, else
    from `tailscale status --json`), which stops DNS rebinding.
32. **What an unauthenticated request sees: nothing.** A bare 403 with no body, for the
    page, its files and the API alike; the refusal is logged (`web.log`, with the login
    if there was one).
33. **CSRF.** No cookies: the identity comes with every request. So a page on another
    site, opened in the person's browser on the tailnet, could post to the page. Each
    POST must have `Content-Type: application/json` and `X-Bullpen: 1` (a cross-site
    form can send neither without a CORS preflight, which is refused) and a
    `Sec-Fetch-Site` of `same-origin` (or an `Origin` that is the page's). GETs change
    nothing.
34. **Headers.** `Content-Security-Policy: default-src 'self'; frame-ancestors 'none'`
    (no inline scripts), `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`,
    `Cache-Control: no-store`, on every answer (the 403s too).
35. **What's sent.** Chat items, never raw records; no attachment, system prompt or
    account record leaves the server.
36. **Tests.** Every test talks to the socket in a temporary state folder, with the
    identity header set and `web_users` and `web_host` set, so no test calls
    `tailscale` or opens a port, and no worker trips its permission checks by starting
    a server on a network address. A test checks the server refuses to start with a
    socket folder others can read.

## Risks and open questions
- **AskUserQuestion's keys.** Answering it on the screen depends on how Claude Code's
  menu takes keys for several questions, multiple choice and "Type something". W0
  checks them in a real pane before W7 builds on them.
- **A transcript's format is Claude Code's**, not a contract. The reader keeps a
  whitelist, hides what it doesn't know, and its tests run on trimmed copies of this
  repository's transcripts; a change shows as a missing line, not a broken page.
- **Polling the screen** costs a `rex capture` a second per watched blocked agent. Rex
  has a server API; an event when a screen changes would replace the poll, if it has
  one.
- **Rex for iPhone** can already show a pane over Tailscale. On the phone, `>_` could
  open the pane there instead. Worth a look once v1 is in use.
- **Mid-turn messages.** Claude Code queues what's typed while it works
  (`queue-operation`); the faint bubble shows that. Whether a queued message should
  wait for the turn's end, as typed news does, is open.
- **Several clients at once** (the Mac and the phone) each get their own stream and see
  the same thing; two presses of the same prompt are kept apart by the prompt's ID, and
  two accepts by the one-action-at-a-time rule per task.
- **Notifications** (Web Push to a home-screen page) would make the phone useful
  without opening it. Later.

## The build
Ten tasks. Wave 1 runs in parallel; each later wave needs the ones it names.

**Wave 1**

- **W0. AskUserQuestion's keys (a check, nothing merged).** In a scratch repository,
  run Claude Code in a Rex pane and have it ask: one question, two questions, a
  multiple-choice one, and one answered with "Type something". Record the screens
  (`rex capture --format=vt`) and the keys that answer each, as fixtures for the fake
  rex. *Result:* a note and fixture files in `briefs/`; `finish`.
- **W1. Know each agent's transcript.** `launch`, the reviewer's start and `up` pass
  `--session-id` to Claude Code and record it (`chat` on the task, its review and the
  coordinator's entry); `resume` and a hand-back use `--resume <id>` (fixes L26);
  `_worker_stop` keeps `transcript_path` for the agent whose turn ended (decision 10),
  as does the coordinator's waiter. Documented in `docs/where-things-live.md`.
  *Tests:* Python, through the fake rex's recorded commands: a started worker's command
  line has a UUID that its task records; a reviewer's differs; a resume passes
  `--resume` with the worker's (never the reviewer's) ID; a non-Claude worker gets
  neither; the worker's turn record has the path; a reviewer's Stop (its session ID,
  or an unknown one under the title `BP-3 review`) leaves the worker's `chat` and turn
  record as they were and moves the review's `chat`.
- **W2. Commands for a coordinator's pane, and drafts.** `prompt`, `answer` and `peek`
  take `--coordinator cN` (with `--repo`), and `prompt`'s JSON adds `draft` (`has_draft`).
  *Tests:* Python, with fake-rex screens: a coordinator at a prompt is read and
  answered; a refused stale prompt; `draft` true with something typed after `❯`, false
  with the dim suggestion.
- **W3. Transcripts into chat items, in core.** `core/src/chat/`: find the transcript
  (recorded ID, else by title), read from an offset to the last whole line, records
  into items as the table above, tool runs grouped, AskUserQuestion as a question item.
  *Tests:* Rust, in `core/src/chat/tests/`, on trimmed copies of this repository's
  transcripts (BP-230's worker and reviewer, BP-232's worker with its compaction, a
  coordinator's with news lines, an AskUserQuestion from a coordinator's) with
  account and e-mail fields removed: the items for each, a half-written last line,
  an unknown record type, a file that grows between two reads.
- **W4. The server: socket, checks, the snapshot, events.** The `web/` crate (after the
  person's yes to the crates), `bullpen web` (in the command table, `docs/commands.md`),
  the socket, the checks in [Security](#security), `/api/fleet` with each task's group,
  `/events` with `fleet` and `ping`, the settings `web_users` and `web_host`
  (`docs/settings.md`), a page that says it's running, and `docs/web.md`. *Tests:*
  Python over the socket: 403 with no login, another login, another host, a POST
  without its header or from another site; the snapshot's groups; a `fleet` event
  within a second of a task file changing; no start with a readable socket folder.

**Wave 2** (W5 after W3 and W4; W6 after W4)

- **W5. Chats over the API.** `/api/chat` and the `chat` event, from W3's reader, with
  the watch on the open transcript only, and "Earlier" paging. *Tests:* Python: a
  fixture transcript in a fake `~/.claude/projects/` served as items; lines appended
  arrive as one `chat` event; nothing raw (no `attachment`, no `session-owner`) in any
  answer. Rust for `markdown.rs`: each element it keeps, and hostile text (`<script>`,
  a `javascript:` link, an image) coming out inert.
- **W6. The page: sidebar, ⌘K, chat.** The three groups, the marks, ⌘K, the chat's
  bubbles, news lines, tool lines and dividers, light and dark, desktop first, as the
  mockups have them (with the front-end design skill). *Tests:* Python: the page and
  its files are served with the CSP, have no inline script and name no other origin.
  The page itself has no test runner in this repository (Rust and Python only), so it
  stays thin: it draws the server's items and posts actions, and the logic it would
  need tests for (grouping, Markdown, escaping) lives in Rust.

**Wave 3** (after W2 and W6; W7 and W8 in parallel)

- **W7. Acting from the page.** The POST routes (tell, say, reply, answer, accept,
  drop, start, resume, tour) through `act.rs`; the prompt cards (permission, a
  coordinator's question, AskUserQuestion with W0's keys); confirms in the board's
  words; Diff and Tour drawers. *Tests:* Python, with fake rex: each route runs its
  command in the right repository with `REX_BLOCK` unset; a second accept while one
  runs is refused; an answer to a changed prompt is refused and says so; a tell into a
  pane with a draft sends nothing.
- **W8. `>_`.** `/api/screen`, the `screen` event while open, `screen.rs`'s SGR spans,
  and the prompt's keys under it. *Tests:* Rust for `screen.rs` (colours, attributes,
  resets, a `<script>` in the text stays text); Python for the polling stopping when
  nobody watches.

**Wave 4** (after W6, W7 and W8)

- **W9. The phone, and polish.** The phone layout, Safari's safe areas and 16-point
  inputs, the stream resuming after the phone sleeps (`Last-Event-ID`), a pass against
  the mockups in both themes. *Tests:* Python for `Last-Event-ID` resuming without a
  gap; the rest is looked at on a phone.

## Yours to decide
Ranked by how much they change what gets built. Answered 2026-11-27, each with the
recommended answer:
1. **The T3 host plan**: set aside (recommended), or build it after web v1.
   *Answered: set aside.*
2. **Mission control**: the page is its rework and it gets no new panels
   (recommended), or both go on. *Answered: the page is its rework; mission control in
   Rex stays as it is, with no new panels.*
3. **The crates for `web/`**: axum, tokio, tokio-stream and notify (recommended), or a
   smaller blocking server with hand-made event streams. *Answered: axum, tokio,
   tokio-stream and notify.*
4. **Who may connect**: the machine's own Tailscale login only (recommended), or a
   list. *Answered: the machine's own login by default, with `web_users` as the
   override decision 30 describes.*
