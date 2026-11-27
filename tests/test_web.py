"""bullpen web: the server on its Unix socket, who it lets in, the snapshot and its
events. Every test talks to the socket in the test's own state folder, with web_users
and web_host set, so none calls `tailscale` or opens a port."""

import datetime as dt
import http.client
import json
import os
import signal
import socket
import subprocess
import time

from support import setUpModule, tearDownModule, BULLPEN, BullpenTest  # a temp folder for each module

ME = "me@example.com"
HOST = "box.tail1.ts.net"
POST = {"Content-Type": "application/json", "X-Bullpen": "1", "Sec-Fetch-Site": "same-origin"}


class UnixConnection(http.client.HTTPConnection):
    """HTTP over a Unix socket, as `tailscale serve` reaches the server."""

    def __init__(self, path, timeout=10):
        super().__init__("localhost", timeout=timeout)
        self.unix = path

    def connect(self):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(self.timeout)
        s.connect(self.unix)
        self.sock = s


class WebTest(BullpenTest):
    def setUp(self):
        super().setUp()
        self.bp("config", "--global", "--web-users", f"someone@example.com, {ME}", "--web-host", HOST)
        self.state = self.tmp / "state"
        self.state.mkdir(exist_ok=True)
        self.state.chmod(0o700)
        self.sock = self.state / "web.sock"

    def serve(self):
        """Start `bullpen web`; returns it, and what it printed first, once it serves."""
        p = subprocess.Popen([str(BULLPEN), "web"], cwd=self.tmp, env=self.env, text=True,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: (p.kill(), p.wait(), p.stdout.close(), p.stderr.close()))
        said = [p.stdout.readline() for _ in range(4)]
        if not said[0]:
            self.fail(f"bullpen web didn't start: {p.stderr.read()}")
        return p, "".join(said)

    def request(self, path, method="GET", headers=None, body=b""):
        """(status, headers, body); a header given as None isn't sent. The request goes
        in one write: a refusal can come, and the connection close, before a body sent
        apart is."""
        h = {"Tailscale-User-Login": ME, "Host": HOST, **(headers or {})}
        if method != "GET":
            h["Content-Length"] = str(len(body))
        lines = [f"{method} {path} HTTP/1.1", *(f"{k}: {v}" for k, v in h.items() if v is not None), "", ""]
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(10)
            s.connect(str(self.sock))
            s.sendall("\r\n".join(lines).encode() + body)
            r = http.client.HTTPResponse(s)
            r.begin()
            return r.status, r.headers, r.read()

    def log(self):
        return (self.state / "web.log").read_text()

    def task_file(self, tid):
        return next(self.state.rglob(f"tasks/{tid}.json"))

    def set_task(self, tid, status, ago=60):
        f = self.task_file(tid)
        since = (dt.datetime.now().astimezone() - dt.timedelta(seconds=ago)).isoformat()
        f.write_text(json.dumps({**json.loads(f.read_text()), "status": status, "since": since}))

    def test_it_serves_on_a_socket_only_you_can_open_and_prints_how_to_reach_it(self):
        p, said = self.serve()
        self.assertEqual(self.sock.stat().st_mode & 0o777, 0o600)
        self.assertIn(f"tailscale serve --bg unix:{self.sock}\n", said)
        self.assertIn(f"https://{HOST}/", said)
        status, headers, body = self.request("/")
        self.assertEqual(status, 200)
        self.assertIn(b"bullpen web is running", body)
        self.assertEqual(headers["Content-Security-Policy"], "default-src 'self'; frame-ancestors 'none'")
        self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
        self.assertEqual(headers["Referrer-Policy"], "no-referrer")
        self.assertEqual(self.request("/api/fleet")[1]["Cache-Control"], "no-store")
        # a second one is refused while this one answers
        again = self.bp("web", ok=False)
        self.assertEqual(again.returncode, 1)
        self.assertIn("already running", again.stderr)
        # one killed leaves its socket behind: the next one takes its place
        p.send_signal(signal.SIGKILL)
        p.wait()
        self.assertTrue(self.sock.exists())
        self.serve()
        self.assertEqual(self.request("/")[0], 200)

    def test_anyone_else_gets_a_bare_403_logged(self):
        self.serve()
        for headers, why in (({"Tailscale-User-Login": None}, "no Tailscale login"),
                             ({"Tailscale-User-Login": "bob@example.com"}, "bob@example.com isn't one of web_users"),
                             ({"Host": "evil.example"}, '"evil.example" isn\'t box.tail1.ts.net')):
            for path in ("/", "/api/fleet", "/events", "/nothing"):
                status, _, body = self.request(path, headers=headers)
                self.assertEqual((status, body), (403, b""), (path, headers))
            self.assertIn(why, self.log())
        self.assertIn("refused GET /api/fleet: bob@example.com isn't one of web_users (login bob@example.com", self.log())

    def test_a_post_must_come_from_the_page_itself(self):
        self.serve()
        # the page's own gets through the checks (there's nothing to post to yet)
        self.assertEqual(self.request("/", "POST", POST, b"{}")[0], 405)
        self.assertEqual(self.request("/", "POST", {**POST, "Sec-Fetch-Site": None,
                                                    "Origin": f"https://{HOST}"}, b"{}")[0], 405)
        for headers in ({"X-Bullpen": None},
                        {"Content-Type": "application/x-www-form-urlencoded"},
                        {"Sec-Fetch-Site": "cross-site", "Origin": "https://evil.example"},
                        {"Sec-Fetch-Site": None}):
            status, _, body = self.request("/", "POST", {**POST, **headers}, b"{}")
            self.assertEqual((status, body), (403, b""), headers)
        self.assertIn("from another site", self.log())
        self.assertIn("no X-Bullpen: 1", self.log())

    def test_the_snapshot_has_each_tasks_group(self):
        self.bp("up")
        for n in range(1, 7):
            self.bp("add", f"Task {n}", "--brief", "Do it.")
        self.set_task("BP-1", "needs")
        self.set_task("BP-2", "ready")
        self.set_task("BP-3", "working")
        self.set_task("BP-4", "done")  # closed a minute ago
        self.set_task("BP-5", "dropped", ago=3 * 24 * 3600)  # three days ago: not in Done
        self.serve()
        status, headers, body = self.request("/api/fleet")
        self.assertEqual(status, 200)
        self.assertEqual(headers["Content-Type"], "application/json")
        repo, = json.loads(body)["repos"]
        groups = {t["task"]["id"]: t["group"] for t in repo["tasks"]}
        self.assertEqual(groups, {"BP-1": "needs", "BP-2": "needs", "BP-3": "active", "BP-6": "active",
                                  "BP-4": "done"})
        self.assertEqual([t["task"]["id"] for t in repo["tasks"]][-1], "BP-4")  # Done after the open ones
        self.assertEqual([(c["id"], c["group"]) for c in repo["coordinators"]], [("c1", "active")])
        self.assertEqual(repo["counts"], {"needs": 1, "ready": 1, "running": 1})

    def test_a_fleet_event_comes_within_a_second_of_a_task_changing(self):
        self.bp("up")
        self.bp("add", "Task", "--brief", "Do it.")
        self.serve()
        c = UnixConnection(str(self.sock), timeout=5)
        c.request("GET", "/events?watch=proj/BP-1", headers={"Tailscale-User-Login": ME, "Host": HOST})
        r = c.getresponse()
        self.addCleanup(c.close)
        self.assertEqual(r.status, 200)
        self.assertEqual(r.headers["Content-Type"], "text/event-stream")

        def event():
            fields = {}
            while (line := r.readline().decode()) not in ("\n", ""):
                k, _, v = line.rstrip("\n").partition(":")
                fields[k] = v.removeprefix(" ")
            return fields

        def status(e):
            return json.loads(e["data"])["repos"][0]["tasks"][0]["task"]["status"]

        first = event()  # on connecting: the snapshot as it is
        self.assertEqual(first["event"], "fleet")
        self.assertTrue(first["id"].isdigit())
        self.assertEqual(status(first), "todo")
        time.sleep(0.3)  # a change now isn't caught up in the start's own
        self.set_task("BP-1", "needs")
        changed = time.monotonic()
        while True:
            e = event()  # a coordinator found alive may come first
            self.assertLess(time.monotonic() - changed, 1.0, "no fleet event within a second of the change")
            if e["event"] == "fleet" and status(e) == "needs":
                break
        self.assertGreater(int(e["id"]), int(first["id"]))

    def test_it_wont_start_with_a_socket_folder_others_can_open(self):
        self.state.chmod(0o755)
        p = self.bp("web", ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn(f"run `chmod 700 {self.state}`", p.stderr)
        self.assertFalse(self.sock.exists())

    def test_it_says_when_it_isnt_built(self):
        self.env["BULLPEN_WEB"] = str(self.tmp / "missing")
        self.assertIn("bullpen web isn't built", self.bp("web", ok=False).stderr)

    def test_its_settings_are_for_every_repository(self):
        p = self.bp("config", "--web-users", ME, ok=False)
        self.assertIn("--web-users is for every repository at once: set it with --global", p.stderr)
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual(cfg["web_host"], HOST)
