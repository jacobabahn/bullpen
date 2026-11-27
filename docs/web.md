# bullpen web

`bullpen web` serves every coordinator and task bullpen runs, across repositories, as a
page for a browser on your tailnet: on a Mac, or on an iPhone in Safari. It's being
built in steps ([the design](prd/web.md)). For now the server is there with its checks:
its page says it's running, `/api/fleet` is the snapshot the page will draw, and
`/events` sends it again whenever it changes. The chats, prompt cards and actions come
next.

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
| `GET /` | a page that says it's running |
| `GET /api/fleet` | the snapshot: `bullpen repos --json`'s shape, with each task's and coordinator's `group`, and the tasks closed in the last two days after the open ones |
| `GET /events` | the event stream: `fleet` (the snapshot, on connecting and whenever it changes; its `id` grows with each), and `ping` every 15 seconds |

A task's `group` is where the page's sidebar puts it: `needs` (it needs you, it
stopped, or it's ready or finished, waiting on your accept or close), `done` (closed in
the last two days) or `active` (the rest). A coordinator is `needs` when it's blocked
at a prompt, else `active`.
