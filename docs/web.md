# bullpen web

`bullpen web` serves every coordinator and task bullpen runs, across repositories, as a
page for a browser on your tailnet: on a Mac, or on an iPhone in Safari. It's being
built in steps ([the design](prd/web.md)). For now the page shows the sidebar, with
⌘K to jump, and each agent's chat, from its transcript ([The page](#the-page),
[Chats](#chats)). Typing to agents, answering prompts and the header's actions come
next: they're drawn, but greyed out.

## Running it

```sh
bullpen web     # in any pane or shell; it runs in the foreground until you stop it
```

It listens on a Unix socket, `~/.local/state/bullpen/web.sock` (the state folder's;
`BULLPEN_STATE` moves it), never on a network port: not the LAN, not the Tailscale
address, not localhost. It prints the line that reaches it from your tailnet, and
never runs it itself:

```sh
tailscale serve --bg unix:/Users/you/.local/state/bullpen/web.sock
```

Run that once: `tailscale serve` keeps it, across restarts, until `tailscale serve
reset`. Tailscale then serves the page over HTTPS at your machine's tailnet name
(`https://box.tail1234.ts.net/`), to your tailnet only. Never use `tailscale funnel`,
which would put it on the internet.

It reads the snapshot again 100 ms after the state folder changes, and once a second in
case a change was missed; which coordinators are alive comes from `bullpen repos --json`
every 5 seconds. When its program is rebuilt (see [Rebuilds](rebuilds.md)) it starts
the new one in its place, and the page's event stream reconnects by itself. Stop it with
ctrl-c; a socket it leaves behind is replaced by the next `bullpen web`, and a second
`bullpen web` while one is running is refused.

## Who can use it

Whoever can use the page can type into agents running as you, so it lets in only you:

- **Only you can reach the socket.** It's made `0600`, in the state folder, which must
  be `0700`: `bullpen web` refuses to start when anyone else can open that folder, and
  says to `chmod 700` it.
- **Only your Tailscale login.** `tailscale serve` adds `Tailscale-User-Login` to every
  request (replacing any the browser sent). The login must be one of `web_users`;
  unset, it's the login this machine is logged in to Tailscale as (`tailscale status
  --json`). A tagged device, or a user shared into your tailnet, has another login or
  none, and is refused.
- **Only your machine's name.** The request's `Host` must be `web_host`; unset, this
  machine's tailnet name, from Tailscale. That stops a page elsewhere reaching it by DNS
  rebinding.
- **Only the page itself changes things.** A request other than a GET must be JSON, with
  `X-Bullpen: 1`, from the page's own origin (`Sec-Fetch-Site: same-origin`, or an
  `Origin` that is the page's), so another site open in your browser can't post to it.
- **Anyone else sees nothing**: a bare 403, no body, for the page, its files and the API
  alike. Each refusal is a line in `~/.local/state/bullpen/web.log` with why, the login
  (if there was one) and the host it asked for: look there when the page won't open.

Every answer has `Content-Security-Policy: default-src 'self'; frame-ancestors 'none'`
(no inline script, nothing from another site, never in a frame),
`X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and `Cache-Control:
no-store`.

Both settings are global, for every repository at once (`config` refuses them without
`--global`), and read when `bullpen web` starts: restart it after changing them.

```sh
bullpen config --global --web-users "you@example.com, you@work.example"   # more than one login
bullpen config --global --web-host box.tail1234.ts.net                    # without asking Tailscale
bullpen config --global --web-users ""                                    # back to your own login
```

With both set, it never asks Tailscale; with either unset and Tailscale not logged in,
it doesn't start and says which to set.

## What it serves

| Route | Returns |
| --- | --- |
| `GET /` | the page (and its `/app.js` and `/app.css`) |
| `GET /api/fleet` | the snapshot: `bullpen repos --json`'s shape, with each task's and coordinator's `group`, and the tasks closed in the last two days after the open ones |
| `GET /api/chat/{repo}/{id}?who=&before=` | an agent's chat: `{agent, source, items, cursor, more}` (below) |
| `GET /events?watch={repo}/{id}[/{who}]&cursor=N` | the event stream: `fleet` (the snapshot, on connecting and whenever it changes; its `id` grows with each), `chat` (the watched chat's new and changed items), and `ping` every 15 seconds |

A task's `group` is where the page's sidebar puts it: `needs` (it needs you, it
stopped, or it's ready or finished, waiting on your accept or close), `done` (closed in
the last two days) or `active` (the rest). A coordinator is `needs` when it's blocked
at a prompt, else `active`.

## The page

The page is three files (`web/src/page/`: `index.html`, `app.js`, `app.css`), built into
the program: plain JavaScript and the system's fonts, nothing fetched from anywhere
else. It draws what the server sends and decides little itself.

- **The sidebar** has every repository's coordinators and the tasks open or closed in
  the last two days, in the server's three groups, each row with the board's mark, its
  words and its repository. **Needs you**: what needs you or stopped, then what's ready
  or finished, then a coordinator blocked at a prompt, the one waiting longest first;
  one that has needed you over 5 minutes has its age in a filled pill. **Active**: a
  repository at a time, its coordinators, then its tasks in review, working, with the
  coordinator, queued. **Done**: closed in the last two days, newest first. The tab's
  title counts what needs you. It updates with each `fleet` event.
- **⌘K** (ctrl-K) finds any row by ID, title or repository; its first row is the next
  that needs you. **n**, anywhere but a text box, goes straight there, and on to the
  next each time.
- **The address** names what's shown: `#bullpen/BP-3` (a task's worker),
  `#bullpen/BP-3/reviewer` (its reviewer, once it has one: the header switches between
  them), `#bullpen/c1` (a coordinator). A link to it opens it.
- **The chat** reads `/api/chat/{repo}/{id}?who=` and then the `chat` events for it: your
  messages and the coordinator's as bubbles (a paste folded to its line count), bullpen's
  news lines (the task each names is a link), what the agent said, each run of tool calls
  as one line that opens on a click, dividers ("Worked for 4 min 38 s", "Conversation
  compacted", "Interrupted"), errors in red, and an AskUserQuestion as a card, read-only
  for now. What the agent said is the item's `html` (escaped by the server), else its
  text as plain text; everything else goes in as text. A server that doesn't serve
  chats yet gets "Chat isn't served yet".
- **Light and dark** follow the system; the sidebar's sun button switches, and the page
  remembers it in that browser.

## Chats

An agent's chat is read from its Claude Code transcript: a task's worker
(`/api/chat/bullpen/BP-3`, or `?who=worker`), its reviewer (`?who=reviewer`), or a
coordinator (`/api/chat/bullpen/c1`), named by its repository's name and its ID. The
transcript is the one whose session ID bullpen recorded when it started the agent (the
task's `chat`, its review's, the coordinator's entry's: see [Where things
live](where-things-live.md)), else the newest in the agent's folder under
`~/.claude/projects/` whose title is the agent's `-n` name (an agent started before
bullpen recorded them).

| Field | What it is |
| --- | --- |
| `agent` | `{repo, id, who, title, mode}`: `who` is `worker`, `reviewer` or `coordinator`; the transcript's title and permission mode, or `null` |
| `source` | how the transcript was found: `recorded`, `title` (matched by title), or `none` (no transcript yet, or an agent that isn't Claude Code: no items) |
| `items` | the last 200 items, oldest first; with `before=ID`, the 200 before that item (a 404 when it isn't one) |
| `cursor` | the byte offset the transcript is read to |
| `more` | whether there are items before these: "Earlier" asks for them with `before` set to the first one's `id` |

An unknown repository, task or coordinator is a 404, and a `who` that isn't `worker` or
`reviewer` (or a coordinator's reviewer) a 400, each with `{error}` saying why.

The items are what [the design](prd/web.md#transcripts-into-chat) lists (`brief`,
`you`, `coordinator`, `news`, `said`, `tools`, `question`, the dividers, `error`,
`queued`), each with its `id`, `at` and `kind`. What the agent said (`said`) comes as
`html`: its Markdown made into HTML by the server, with paragraphs, emphasis, inline
and fenced code, lists, tables and links to `http` and `https` addresses only.
Everything else in it is escaped text: raw HTML, a `<script>`, an image (its alt text
alone, never fetched), a link to anything but a web page (its text alone). A heading is
a paragraph in bold. The rest of an item's text is plain text, for the page to set as
text. No record leaves the server: nothing from an attachment, the session's owner, a
system prompt, an account or an e-mail address, and an AskUserQuestion's questions and
answers carry their own fields alone.

A transcript is read whole the first time anyone asks for it (about 100 ms for 20 MB),
kept while the server runs, and read on from its offset after that, to its last whole
line (a line still being written waits for the next read).

### The `chat` event

`/events?watch=bullpen/BP-3` (or `…/BP-3/reviewer`, `…/c1`) watches that chat's
transcript, and only that one. A change to it is read within about 30 ms (100 ms at
most while writes keep coming), and every watched transcript is read once a second
whatever was heard, in case a change was missed. What a read made or changed comes as
one `chat` event, `{items, cursor}`: new items, and items that changed, by their `id` (a
tool line whose call finished, a question answered, a queued message taken in). The
page puts each where the item with its `id` is, or at the end.

`cursor=N` is the `cursor` of the page of the chat the page has: on connecting, the
stream first sends what changed since then, so nothing written between the two
requests is missed. When the chat is to be drawn again from the start, the event is the
whole chat as `/api/chat` answers it, with `reset: true`: its agent's conversation
moved to another transcript (its Stop hook moved its `chat` after a `/clear`), one was
found where there was none, the file was written anew, the stream fell too far behind,
or its `cursor` is from before this server read the file (the page's stream reconnects
after `bullpen web` restarts, on a rebuild) or past where it has read to.
