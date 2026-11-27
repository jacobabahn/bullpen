"""bullpen web's chats: `/api/chat` and the `chat` event, from transcripts in a fake
~/.claude/projects/ in the test's folder (the server's home is the test's), never the
real one. Every test talks to the socket in the test's own state folder."""

import json
import re
import time
import uuid

from support import setUpModule, tearDownModule  # a temp folder for each module
from test_web import HOST, ME, UnixConnection, WebCase

# what records carry that must never leave the server
RAW = ("SECRET-ATTACHMENT", "owner@example.com", "acct-4242", "SYSTEM-PROMPT-TEXT", "session-owner",
       "api-request", "toolUseResult", "isSidechain", "SIDECHAIN-WORDS")


def rec(kind, **fields):
    return {"type": kind, "uuid": str(uuid.uuid4()), "timestamp": "2026-11-27T10:00:00.000Z",
            "sessionId": "s", **fields}


def typed(text):
    return rec("user", message={"role": "user", "content": text})


def said(text, msg=None):
    return rec("assistant", message={"id": msg or f"msg_{uuid.uuid4().hex}", "role": "assistant",
                                     "content": [{"type": "text", "text": text}]})


def call(cid, command):
    return rec("assistant", message={"id": f"msg_{cid}", "role": "assistant", "content": [
        {"type": "tool_use", "id": cid, "name": "Bash", "input": {"command": command}}]})


def result(cid, out):
    return rec("user", message={"role": "user", "content": [
        {"tool_use_id": cid, "type": "tool_result", "content": out}]},
        toolUseResult={"stdout": out, "stderr": ""})


def opening(title):
    """What Claude Code writes first, with the bookkeeping records that carry what's private."""
    return [
        {"type": "custom-title", "customTitle": title, "sessionId": "s"},
        {"type": "permission-mode", "permissionMode": "auto", "sessionId": "s"},
        {"type": "session-owner", "sessionId": "s", "email": "owner@example.com", "accountUuid": "acct-4242"},
        rec("attachment", attachment={"type": "environment", "content": "SECRET-ATTACHMENT"}),
        {"type": "api-request", "sessionId": "s", "system": "SYSTEM-PROMPT-TEXT"},
        {**said("SIDECHAIN-WORDS"), "isSidechain": True},
    ]


class ChatTest(WebCase):
    def setUp(self):
        super().setUp()
        self.home = self.tmp / "home"
        self.env["HOME"] = str(self.home)  # the server's ~/.claude/projects is the test's
        self.env.pop("CLAUDE_CONFIG_DIR", None)
        self.projects = self.home / ".claude" / "projects"
        self.bp("up")
        self.bp("add", "Task", "--brief", "Do it.")
        self.worktree = self.tmp / "wt" / "BP-1"
        self.chat = str(uuid.uuid4())
        self.set_fields("BP-1", worktree=str(self.worktree), chat=self.chat)

    def set_fields(self, tid, **fields):
        f = self.task_file(tid)
        f.write_text(json.dumps({**json.loads(f.read_text()), **fields}))

    def transcript(self, name, records, cwd=None):
        """A transcript in the folder Claude Code keeps cwd's in (the worktree's by default)."""
        folder = re.sub(r"[^A-Za-z0-9]", "-", str(cwd or self.worktree))
        path = self.projects / folder / f"{name}.jsonl"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("".join(json.dumps(r) + "\n" for r in records))
        return path

    def append(self, path, records):
        with path.open("a") as f:
            f.write("".join(json.dumps(r) + "\n" for r in records))  # one write

    def chat_of(self, path):
        status, headers, body = self.request(path)
        self.assertEqual(status, 200, body)
        self.assertEqual(headers["Content-Type"], "application/json")
        self.raw_free(body.decode())
        return json.loads(body)

    def raw_free(self, text):
        for raw in RAW:
            self.assertNotIn(raw, text)

    def worker_transcript(self):
        return self.transcript(self.chat, [
            *opening("BP-1: Task"),
            typed("You are the worker for bullpen task BP-1. Read your brief at /s/briefs/BP-1.full.md and follow it."),
            said("I'll **look** first: <script>alert(1)</script>"),
            call("toolu_1", "ls"),
            result("toolu_1", "README"),
            call("toolu_2", "python3 tests/run.py"),
        ])

    def events(self, query):
        """The event stream for query: a function that reads its next event's fields."""
        c = UnixConnection(str(self.sock), timeout=5)
        c.request("GET", f"/events?{query}", headers={"Tailscale-User-Login": ME, "Host": HOST})
        r = c.getresponse()
        self.addCleanup(c.close)
        self.assertEqual(r.status, 200)

        def event():
            fields = {}
            while (line := r.readline().decode()) not in ("\n", ""):
                k, _, v = line.rstrip("\n").partition(":")
                fields[k] = v.removeprefix(" ")
            return fields

        return event, c

    def next_chat(self, event, within=3.0):
        deadline = time.monotonic() + within
        while time.monotonic() < deadline:
            e = event()
            if e.get("event") == "chat":
                self.raw_free(e["data"])
                return json.loads(e["data"])
        self.fail("no chat event")

    def test_a_workers_transcript_is_served_as_items(self):
        path = self.worker_transcript()
        self.serve()
        chat = self.chat_of("/api/chat/proj/BP-1")
        self.assertEqual(chat["agent"], {"repo": "proj", "id": "BP-1", "who": "worker", "title": "BP-1: Task",
                                         "mode": "auto"})
        self.assertEqual((chat["source"], chat["more"], chat["cursor"]), ("recorded", False, path.stat().st_size))
        self.assertEqual([i["kind"] for i in chat["items"]], ["brief", "said", "tools"])
        brief, words, tools = chat["items"]
        self.assertEqual(brief["file"], "BP-1.full.md")
        # what it said is HTML, escaped, never its Markdown
        self.assertNotIn("text", words)
        self.assertEqual(words["html"], "<p>I&#39;ll <strong>look</strong> first: "
                                        "&lt;script&gt;alert(1)&lt;/script&gt;</p>")
        self.assertEqual([(c["what"], c["state"]) for c in tools["calls"]],
                         [("ls", "done"), ("python3 tests/run.py", "running")])
        self.assertEqual(self.chat_of("/api/chat/proj/BP-1?who=worker"), chat)

    def test_a_reviewers_and_a_coordinators_chats(self):
        self.worker_transcript()
        # the reviewer, started before bullpen recorded its chat: matched by its title
        self.transcript(str(uuid.uuid4()), [*opening("BP-1 review"), said("Looks *fine*.")])
        cfg = json.loads(next(self.state.rglob("repos/*/config.json")).read_text())
        coordinator = cfg["coordinators"][0]
        self.assertTrue(coordinator.get("chat"), cfg)  # `up` recorded it
        self.transcript(coordinator["chat"], [*opening("bullpen: proj"), typed("bullpen · BP-1 is ready.")],
                        cwd=self.repo)
        self.serve()
        review = self.chat_of("/api/chat/proj/BP-1?who=reviewer")
        self.assertEqual((review["agent"]["who"], review["source"]), ("reviewer", "title"))
        self.assertEqual([i.get("html") for i in review["items"]], ["<p>Looks <em>fine</em>.</p>"])
        c1 = self.chat_of("/api/chat/proj/c1")
        self.assertEqual((c1["agent"]["who"], c1["agent"]["title"], c1["source"]),
                         ("coordinator", "bullpen: proj", "recorded"))
        self.assertEqual([(i["kind"], i["text"], i["task"]) for i in c1["items"]], [("news", "BP-1 is ready.", "BP-1")])
        # a task with no transcript yet has an empty chat
        self.bp("add", "Another", "--brief", "Do it.")
        deadline = time.monotonic() + 3
        while self.request("/api/chat/proj/BP-2")[0] == 404 and time.monotonic() < deadline:
            time.sleep(0.05)  # the snapshot is read again
        none = self.chat_of("/api/chat/proj/BP-2")
        self.assertEqual((none["source"], none["items"], none["cursor"], none["more"]), ("none", [], 0, False))

    def test_what_isnt_there_or_is_asked_wrongly_is_refused(self):
        self.serve()
        for path, status, error in (("/api/chat/proj/BP-9", 404, "no task or coordinator BP-9 in proj"),
                                    ("/api/chat/nope/BP-1", 404, "no repo named nope"),
                                    ("/api/chat/proj/BP-1?who=boss", 400, 'who is worker or reviewer, not "boss"'),
                                    ("/api/chat/proj/c1?who=reviewer", 400, "c1 is a coordinator: it has no reviewer")):
            got, _, body = self.request(path)
            self.assertEqual((got, json.loads(body)), (status, {"error": error}), path)

    def test_earlier_pages_back_200_items_at_a_time(self):
        self.transcript(self.chat, [*opening("BP-1: Task"), *(typed(f"message {n}") for n in range(1, 451))])
        self.serve()
        last = self.chat_of("/api/chat/proj/BP-1")
        texts = [i["text"] for i in last["items"]]
        self.assertEqual((len(texts), texts[0], texts[-1], last["more"]), (200, "message 251", "message 450", True))
        before = self.chat_of(f"/api/chat/proj/BP-1?before={last['items'][0]['id']}")
        texts = [i["text"] for i in before["items"]]
        self.assertEqual((len(texts), texts[0], texts[-1], before["more"]), (200, "message 51", "message 250", True))
        first = self.chat_of(f"/api/chat/proj/BP-1?before={before['items'][0]['id']}")
        texts = [i["text"] for i in first["items"]]
        self.assertEqual((len(texts), texts[0], texts[-1], first["more"]), (50, "message 1", "message 50", False))
        self.assertEqual(first["cursor"], last["cursor"])
        status, _, body = self.request("/api/chat/proj/BP-1?before=nothing")
        self.assertEqual((status, json.loads(body)), (404, {"error": "no item nothing in this chat"}))

    def test_lines_appended_arrive_as_one_chat_event(self):
        path = self.worker_transcript()
        other = self.transcript(str(uuid.uuid4()), [*opening("BP-1 review")])  # in the same folder, not watched
        self.serve()
        chat = self.chat_of("/api/chat/proj/BP-1")
        tools = chat["items"][-1]
        event, conn = self.events(f"watch=proj/BP-1&cursor={chat['cursor']}")
        self.assertEqual(event()["event"], "fleet")
        self.append(other, [said("The reviewer's words.")])
        self.append(path, [result("toolu_2", "OK"), *opening("BP-1: Task"), said("All **green**.")])
        e = self.next_chat(event)
        self.assertEqual(e["cursor"], path.stat().st_size)
        # the tool line, finished, by its id; then what was said
        self.assertEqual([(i["kind"], i["id"]) for i in e["items"]], [("tools", tools["id"]), ("said", e["items"][1]["id"])])
        self.assertEqual([c["state"] for c in e["items"][0]["calls"]], ["done", "done"])
        self.assertEqual(e["items"][1]["html"], "<p>All <strong>green</strong>.</p>")
        # and nothing more: the next event within a second isn't another chat
        conn.sock.settimeout(1)
        with self.assertRaises(TimeoutError):
            while True:
                self.assertNotEqual(event().get("event"), "chat")

    def test_a_stream_sends_what_changed_since_its_pages_cursor(self):
        path = self.worker_transcript()
        self.serve()
        chat = self.chat_of("/api/chat/proj/BP-1")
        self.append(path, [said("Said before the stream opened.")])
        time.sleep(0.2)
        self.chat_of("/api/chat/proj/BP-1?before=" + chat["items"][0]["id"])  # another read: the file is read on
        event, _ = self.events(f"watch=proj/BP-1&cursor={chat['cursor']}")
        e = self.next_chat(event)
        self.assertEqual([i.get("html") for i in e["items"]], ["<p>Said before the stream opened.</p>"])

    def test_a_chat_that_moves_is_sent_whole(self):
        self.worker_transcript()
        self.serve()
        event, _ = self.events("watch=proj/BP-1")
        self.assertEqual(event()["event"], "fleet")
        time.sleep(0.3)  # the stream is following the first transcript
        fresh = str(uuid.uuid4())  # the worker's /clear: its Stop hook moves its chat
        self.transcript(fresh, [*opening("BP-1: Task"), said("A new conversation.")])
        self.set_fields("BP-1", chat=fresh)
        e = self.next_chat(event)
        self.assertEqual((e["reset"], e["source"], e["more"]), (True, "recorded", False))
        self.assertEqual([i.get("html") for i in e["items"]], ["<p>A new conversation.</p>"])
