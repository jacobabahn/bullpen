// bullpen web's page (docs/prd/web.md): the sidebar from /api/fleet and its `fleet`
// events, ⌘K to jump, and the chat the hash names (#repo/BP-3, #repo/BP-3/reviewer,
// #repo/c1) from /api/chat and `chat` events. It draws what the server sends and
// decides nothing worth a test: the groups, the board's words and the chat items are
// the server's. What an agent said comes as HTML the server escaped and goes in as it
// is; everything else goes in as text. The composer and the header's actions are drawn
// but not wired yet.
"use strict";

(function () {
  const $ = (id) => document.getElementById(id);

  // ---------- state ----------
  let fleet = null; // the latest snapshot
  let rows = []; // the sidebar's rows, in their groups' order
  let live = false; // the event stream is open
  let stream = null;
  let retry; // the timer that opens a refused stream again
  let shown; // the key of what's shown (keyOf), once something is
  let chat = null; // the chat shown: {key, state, items, more, source, agent, error}
  let pending = []; // chat events heard before the chat's first read came
  const opened = new Set(); // the tool lines opened, by item ID
  const pal = { on: false, q: "", i: 0 };

  // ---------- small helpers ----------
  /** An element: h("div", {class: "x"}, child, "text", …); text is always text. */
  function h(tag, attrs, ...kids) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v === false || v == null) continue;
      if (k === "class") el.className = v;
      else el.setAttribute(k, v === true ? "" : v);
    }
    for (const k of kids.flat()) {
      if (k == null || k === false) continue;
      el.append(k instanceof Node ? k : document.createTextNode(String(k)));
    }
    return el;
  }

  /** Our own SVG (never data) as an element. */
  function svg(markup) {
    const t = document.createElement("template");
    t.innerHTML = markup.trim();
    return t.content.firstChild;
  }

  const ICON = {
    chev: '<svg class="chev" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M3.5 2l3 3-3 3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    up: '<svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 13V3.5M3.8 7.5L8 3.3l4.2 4.2" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    back: '<svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><path d="M12.5 4l-6 6 6 6" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    doc: '<svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 1.8h5.2L12.5 5v9.2H4z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>',
  };

  // The board's marks, drawn: a task's by its status, a coordinator's by its word.
  const RING = (cls) => `<circle cx="8" cy="8" r="5" class="${cls}" stroke-width="2"/>`;
  const ARC = (cls, d) => `<g class="spin"><path d="${d}" class="${cls}" stroke-width="2" stroke-linecap="round"/></g>`;
  const MARKS = {
    needs: '<circle cx="8" cy="8" r="5" class="f-amber"/>',
    stopped: '<rect x="3" y="3" width="10" height="10" rx="2.5" class="f-red"/>',
    triage: '<circle cx="8" cy="8" r="5.2" class="s-ink2" stroke-width="1.5"/><circle cx="8" cy="8" r="2.2" class="s-ink2" stroke-width="1.5"/>',
    ready: '<path d="M3.5 8.4l3 3 6-6.6" class="s-green" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>',
    finished: '<path d="M8 2.5l5.5 5.5L8 13.5 2.5 8z" class="f-green"/>',
    review: RING("s-blue-fill") + ARC("s-blue", "M8 3a5 5 0 0 1 5 5"),
    working: RING("s-line2") + ARC("s-ink2", "M8 3a5 5 0 0 1 4.3 2.5"),
    todo: '<circle cx="8" cy="8" r="5" class="s-ink3" stroke-width="1.6" stroke-dasharray="1.6 2.3"/>',
    done: '<circle cx="8" cy="8" r="2.2" class="f-ink3"/>',
    dropped: '<path d="M4.5 8h7" class="s-ink3" stroke-width="1.8" stroke-linecap="round"/>',
    "c-working": RING("s-line2") + ARC("s-ink2", "M8 3a5 5 0 0 1 4.3 2.5"),
    "c-idle": '<circle cx="8" cy="8" r="5" class="s-ink3" stroke-width="1.6"/>',
    "c-blocked": '<circle cx="8" cy="8" r="5" class="f-amber"/>',
    "c-unknown": '<circle cx="8" cy="8" r="5" class="s-ink3" stroke-width="1.6" stroke-dasharray="1.6 2.3"/>', // the board's: todo's dotted ring
    "c-gone": '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" class="s-amber" stroke-width="2" stroke-linecap="round"/>',
  };
  function mark(kind) {
    const body = MARKS[kind] || MARKS.done;
    return svg(`<svg class="m" viewBox="0 0 16 16" aria-hidden="true">${body}</svg>`);
  }

  const STATUS = {
    needs: "Needs you", stopped: "Stopped", triage: "Coordinator on it", ready: "Ready to accept",
    finished: "Finished", review: "In review", working: "Working", todo: "Queued", done: "Done", dropped: "Dropped",
  };
  const COORD = { working: "Working", idle: "Idle", blocked: "Blocked at a prompt", gone: "Gone", unknown: "Not known" };
  const upper = (s) => (s ? s[0].toUpperCase() + s.slice(1) : s);

  /** "now", "6m", "2h", "1d", as the board says an age. */
  function ago(ms) {
    if (!ms) return "";
    const s = Math.max(0, (Date.now() - ms) / 1000);
    for (const [u, n] of [["d", 86400], ["h", 3600], ["m", 60]]) if (s >= n) return Math.floor(s / n) + u;
    return "now";
  }
  /** A length of time as the chat's items say it: "26 s", "4 min 38 s", "1 h 29 min". */
  function span(ms) {
    const t = Math.floor((ms || 0) / 1000), hr = Math.floor(t / 3600), m = Math.floor(t / 60) % 60, s = t % 60;
    if (!hr && !m) return `${s} s`;
    if (!hr) return m < 10 && s ? `${m} min ${s} s` : `${m} min`;
    return m ? `${hr} h ${m} min` : `${hr} h`;
  }
  function clock(at) {
    const d = new Date(at);
    if (isNaN(d)) return "";
    const t = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return d.toDateString() === new Date().toDateString() ? t : `${d.toLocaleDateString([], { day: "numeric", month: "short" })}, ${t}`;
  }
  const stamp = (s) => (s ? Date.parse(s) || 0 : 0);

  // ---------- where we are: the hash ----------
  /** {repo, id, who} from #repo/ID[/reviewer], or null. */
  function route() {
    const parts = location.hash.slice(1).split("/").filter(Boolean).map(decodeURIComponent);
    if (parts.length < 2) return null;
    return { repo: parts[0], id: parts[1], who: parts[2] === "reviewer" ? "reviewer" : "worker" };
  }
  function hashOf(repo, id, who) {
    return "#" + [repo, id].map(encodeURIComponent).join("/") + (who === "reviewer" ? "/reviewer" : "");
  }
  const keyOf = (r) => r && `${r.repo}/${r.id}/${r.who}`;

  // ---------- the sidebar ----------
  const NEEDS_ORDER = { needs: 0, stopped: 0, ready: 1, finished: 2 };
  const ACTIVE_ORDER = { review: 0, working: 1, triage: 2, todo: 3 };

  function taskRow(repo, t) {
    const task = t.task || {};
    const st = task.status || "";
    const since = stamp(task.since || task.updated);
    const awaits = st === "ready" || st === "finished";
    return {
      kind: "task", repo: repo.name, id: task.id, group: t.group, status: st, mark: st, task,
      title: task.title || "", line: t.words || "", since,
      overdue: st === "needs" && since && Date.now() - since > 5 * 60 * 1000,
      risk: awaits && task.risk && task.risk.level,
      coordinator: task.coordinator,
    };
  }
  function coordRow(repo, c) {
    const word = c.word || "idle";
    return {
      kind: "coordinator", repo: repo.name, id: c.id, group: c.group, status: word, mark: "c-" + word, coord: c,
      title: `${repo.name}'s coordinator`, line: COORD[word] + (c.text ? `: ${c.text}` : ""),
      since: c.at ? c.at * 1000 : 0,
    };
  }

  /** The rows in their groups, across repositories: Needs you longest-waiting first (tasks
   * that need you, then ready, finished, then blocked coordinators); Active a repository
   * at a time, its coordinators then its tasks (in review, working, the coordinator's,
   * queued); Done newest first. */
  function rowsOf(f) {
    const needs = [], active = [], done = [];
    (f.repos || []).forEach((repo, ri) => {
      for (const c of repo.coordinators || []) {
        const r = coordRow(repo, c);
        (r.group === "needs" ? needs : active).push([r, ri, -1]);
      }
      (repo.tasks || []).forEach((t, ti) => {
        const r = taskRow(repo, t);
        ({ needs, done }[r.group] || active).push([r, ri, ti]);
      });
    });
    const rank = (r, order) => (r.kind === "coordinator" ? (order === NEEDS_ORDER ? 3 : -1) : order[r.status] ?? 4);
    needs.sort((a, b) => rank(a[0], NEEDS_ORDER) - rank(b[0], NEEDS_ORDER) || (a[0].since || Infinity) - (b[0].since || Infinity));
    active.sort((a, b) => a[1] - b[1] || rank(a[0], ACTIVE_ORDER) - rank(b[0], ACTIVE_ORDER) || a[2] - b[2]);
    done.sort((a, b) => b[0].since - a[0].since);
    return [...needs, ...active, ...done].map((x) => x[0]);
  }

  const GROUPS = [["needs", "Needs you"], ["active", "Active"], ["done", "Done today and yesterday"]];

  function drawSide() {
    const sel = route();
    const box = $("groups");
    const top = box.scrollTop;
    box.replaceChildren();
    if (!fleet) {
      box.append(h("div", { class: "side-note" }, "Reading what bullpen runs…"));
    } else if (fleet.error) {
      box.append(h("div", { class: "side-note bad" }, `bullpen's state can't be read: ${fleet.error}`));
    } else if (!rows.length) {
      box.append(h("div", { class: "side-note" }, "Nothing yet. Run bullpen up in a repository to start its coordinator."));
    }
    for (const [g, name] of GROUPS) {
      const these = rows.filter((r) => r.group === g);
      if (!these.length) continue;
      box.append(h("div", { class: `group-h ${g}` }, h("span", {}, name), h("span", { class: "n" }, these.length)));
      for (const r of these) box.append(sideRow(r, sel));
    }
    box.scrollTop = top;

    const needs = rows.filter((r) => r.group === "needs");
    const wanting = needs.filter((r) => r.kind === "coordinator" || NEEDS_ORDER[r.status] === 0).length;
    const ready = needs.length - wanting;
    $("counts").replaceChildren(
      wanting ? h("span", { class: "chip amber", title: `${wanting} ${wanting === 1 ? "needs" : "need"} you` }, h("span", { class: "dot" }), wanting) : "",
      ready ? h("span", { class: "chip green", title: `${ready} ready or finished` }, `✓ ${ready}`) : "",
    );
    document.title = needs.length ? `${needs.length} ${needs.length === 1 ? "needs" : "need"} you · bullpen` : "bullpen";
    const n = fleet && fleet.repos ? fleet.repos.length : 0;
    $("live").classList.toggle("on", live);
    $("live-words").textContent = live ? `Live · ${n} ${n === 1 ? "repository" : "repositories"}` : "Reconnecting…";
  }

  function isSel(r, sel) {
    return sel && sel.repo === r.repo && sel.id === r.id;
  }

  function sideRow(r, sel) {
    const on = isSel(r, sel);
    const cls = ["row", r.status === "needs" || r.mark === "c-blocked" ? "wash" : "", on ? "sel" : "",
      r.group === "done" ? "dim" : "", r.kind === "coordinator" ? "coord" : ""].filter(Boolean).join(" ");
    const age = ago(r.since);
    return h("a", { class: cls, href: hashOf(r.repo, r.id, on ? sel.who : "worker"), "aria-current": on ? "page" : false },
      h("span", { class: "mk" }, mark(r.mark)),
      h("span", {},
        h("span", { class: "l1" }, h("span", { class: "id" }, r.id), h("span", { class: "repo" }, r.repo),
          age ? h("span", { class: r.overdue ? "age over" : "age", title: r.overdue ? "Waiting over 5 minutes" : false }, age) : ""),
        h("span", { class: "t" }, r.title),
        h("span", { class: "d" }, h("span", {}, r.line), r.risk ? risk(r.risk, "", r.task.risk.why) : "")));
  }

  function risk(level, suffix, why) {
    return h("span", { class: `risk ${level}`, title: why || false }, h("i"), level + suffix);
  }

  // ---------- the chat: header ----------
  const LATER = "Not wired up yet: use the board for now";

  function header(sel, r) {
    const isTask = !r || r.kind === "task";
    const t = (r && r.task) || {};
    const st = r ? r.status : "";
    const hasReview = isTask && t.review && typeof t.review === "object";
    const who = hasReview ? sel.who : "worker";
    const role = !isTask ? "Coordinator" : who === "reviewer" ? "Reviewer" : "Worker";
    const model = who === "reviewer" ? hasReview && t.review.model : t.worker_model || t.model;
    const l3 = [h("span", { class: `status ${statusClass(st)}` }, r ? (isTask ? STATUS[st] || st : COORD[st]) : "Not running now")];
    if (st === "ready" || st === "finished") {
      if (typeof t.pr === "string" && t.pr) l3.push(prLink(t.pr));
      else if (t.local) l3.push(h("span", { class: "wh" }, "Local branch"));
      if (t.diffstat) l3.push(h("span", { class: "wh num" }, t.diffstat));
    }
    if (hasReview) {
      l3.push(h("span", { class: "seg", role: "group", "aria-label": "Whose chat" },
        ["worker", "reviewer"].map((w) => h("a", { href: hashOf(sel.repo, sel.id, w), "aria-current": w === who ? "page" : false }, upper(w)))));
    }
    l3.push(h("span", { class: "seg", role: "group", "aria-label": "View" },
      h("a", { href: hashOf(sel.repo, sel.id, who), "aria-current": "page" }, "Chat"),
      h("button", { type: "button", disabled: true, title: "The pane's screen isn't served yet" }, h("span", { class: "mono" }, ">_"))));
    return h("header", { class: "head" },
      h("a", { class: "back", href: "#", "aria-label": "All coordinators and tasks" }, svg(ICON.back)),
      h("div", { class: "head-main" },
        h("div", { class: "head-l1" }, mark(r ? r.mark : "done"), h("span", { class: "id" }, sel.id), h("span", {}, sel.repo),
          h("span", { class: "chip quiet" }, role + (model ? ` · ${upper(model)}` : ""))),
        h("h1", { class: "head-title" }, r ? r.title : sel.id),
        h("div", { class: "head-l3" }, l3)),
      h("div", { class: "actions" }, actions(r)));
  }

  /** A task's pull request (its `pr` is the URL) as a link: "Pull request #16", as the
   * board's pr_short reads the number. */
  function prLink(url) {
    const n = (url.match(/\/pull\/(\d+)/) || [])[1];
    const safe = /^https?:\/\//.test(url);
    return h(safe ? "a" : "span", { class: "wh", href: safe ? url : false, target: safe ? "_blank" : false, rel: safe ? "noreferrer" : false },
      n ? `Pull request #${n}` : "Pull request");
  }

  function statusClass(st) {
    return { needs: "needs", blocked: "needs", stopped: "stopped", ready: "ready", finished: "ready", review: "review" }[st] || "";
  }

  /** The header's buttons, drawn but disabled until the page can act. */
  function actions(r) {
    const b = (label, cls) => h("button", { class: `btn ${cls || ""}`, type: "button", disabled: true, title: LATER }, label);
    if (!r) return [];
    if (r.kind === "coordinator") return [b("Open in Rex")];
    const out = [];
    if (r.status === "ready" || r.status === "finished") {
      if (r.task.risk && r.task.risk.level) out.push(risk(r.task.risk.level, " risk", r.task.risk.why));
      out.push(b("Diff"), b("Tour"), b("Drop", "ghost danger"), b("Accept", "go"));
    } else if (r.status === "todo") out.push(b("Start", "primary"));
    else if (r.status === "stopped") out.push(b("Resume", "primary"));
    else if (r.group !== "done") out.push(b("Open in Rex"));
    return out;
  }

  // ---------- the chat: items ----------
  function parts(item) {
    const ps = item.parts || [{ text: item.text || "" }];
    return ps.map((p) => p.pasted != null
      ? h("div", { class: "paste" }, h("span", { class: "ic" }, "TXT"),
        h("span", {}, h("b", {}, `Pasted text, ${p.lines} ${p.lines === 1 ? "line" : "lines"}`), (p.pasted.split("\n")[0] || "").slice(0, 200)))
      : h("span", {}, p.text));
  }

  function newsItem(item, sel) {
    const [first, ...rest] = (item.text || "").split("\n");
    const target = item.task && rows.find((r) => r.kind === "task" && r.repo === sel.repo && r.id === item.task);
    return h("div", { class: "news" },
      h("span", { class: "mk" }, mark(target ? target.mark : "done")),
      h("span", {}, h("span", { class: "from" }, "bullpen"),
        target ? h("a", { href: hashOf(sel.repo, item.task, "worker") }, first) : first,
        rest.length ? h("pre", {}, rest.join("\n")) : ""));
  }

  function toolsItem(item) {
    const open = opened.has(item.id);
    const head = item.doing
      ? [mark("working"), h("span", { class: "run" }, item.doing)]
      : [svg(ICON.chev), h("span", {}, item.summary || "Used tools")];
    return h("div", { class: open ? "tools msg open" : "tools msg", "data-tools": item.id },
      h("button", { type: "button", "aria-expanded": String(open) }, head, item.ms >= 1000 ? h("span", { class: "dur" }, span(item.ms)) : ""),
      h("div", { class: "calls" }, (item.calls || []).map(callLine)));
  }

  const WORDS = new Set(["Agent", "Task", "WebSearch", "Skill", "ToolSearch"]);
  /** Is what a call did in words (an agent's task, Bash's description), not code? A Bash
   * call with a description says it as it is while it runs (items.rs, doing). */
  const words = (c) => WORDS.has(c.name) || (c.name === "Bash" && !!c.what && c.doing === c.what);
  function callLine(c) {
    const st = c.state === "done" ? h("span", { class: "st ok", title: "Done" }, "✓")
      : c.state === "failed" ? h("span", { class: "st bad" }, "failed")
      : h("span", { class: "st", title: "Running" }, "…");
    return h("div", { class: "call" },
      h("div", { class: "top" }, h("span", { class: "tn", title: c.name }, c.name),
        h("span", { class: words(c) ? "lab words" : "lab", title: c.what || false }, c.what || c.doing || ""), st),
      c.error ? h("div", { class: "out err" }, c.error) : "",
      c.tail ? h("div", { class: "out" }, c.tail) : "");
  }

  /** An AskUserQuestion as a card, read-only: each question, its options, and once it
   * has its answer, what was chosen. */
  function questionItem(item, sel) {
    const qs = Array.isArray(item.questions) ? item.questions : [];
    const answers = item.answers && typeof item.answers === "object" ? item.answers : null;
    const done = !!(answers || item.error);
    const body = [];
    if (item.error) body.push(h("p", { class: "bad" }, item.error));
    for (const q of qs) {
      const chosen = answers ? String(answers[q.question] ?? "") : "";
      if (q.header) body.push(h("div", { class: "qtabs" }, h("span", { class: "qtab" }, q.header)));
      body.push(h("div", { class: "q" }, q.question || ""));
      const opts = (q.options || []).map((o, j) => {
        const label = typeof o === "string" ? o : o.label || "";
        const picked = chosen && chosen.split(", ").includes(label);
        return h("button", { class: "opt", type: "button", disabled: true, "aria-pressed": String(!!picked) },
          h("span", { class: "kbd" }, j + 1), h("span", {}, label, o.description ? h("small", {}, o.description) : ""));
      });
      body.push(h("div", { class: "opts" }, opts));
      if (chosen && !opts.some((o) => o.getAttribute("aria-pressed") === "true")) {
        body.push(h("p", { class: "plain" }, `Answered: ${chosen}`));
      }
    }
    const who = sel.id;
    return h("section", { class: done ? "card msg answered" : "card msg", "aria-label": `${who} asks` },
      h("div", { class: "card-h" }, mark(done ? "ready" : "needs"),
        h("span", {}, done ? `${who} asked` : qs.length > 1 ? `${who} asks ${qs.length} questions` : `${who} asks`),
        h("span", { class: "src" }, "AskUserQuestion, from the transcript")),
      h("div", { class: "card-b" }, body),
      done ? "" : h("div", { class: "card-f" }, h("span", {}, "Answer it in its pane for now: the page can't press its keys yet.")));
  }

  function saidItem(item) {
    const el = h("div", { class: "msg say" });
    if (typeof item.html === "string") {
      el.innerHTML = item.html; // escaped by the server's markdown.rs
    } else {
      for (const p of (item.text || "").split(/\n{2,}/)) el.append(h("p", { class: "plain" }, p));
    }
    return el;
  }

  function itemEl(item, sel) {
    switch (item.kind) {
      case "brief":
        return h("div", { class: "brief" }, svg(ICON.doc),
          h("span", {}, item.file ? "Started from its brief" : "Started by bullpen"),
          item.file ? h("span", { class: "file" }, item.file) : "", h("span", { class: "num" }, clock(item.at)));
      case "you":
        return h("div", { class: "msg me" }, h("div", {}, h("div", { class: "bubble" }, parts(item)), h("div", { class: "meta num" }, clock(item.at))));
      case "coordinator": {
        const r = rows.find((x) => x.kind === "task" && x.repo === sel.repo && x.id === sel.id);
        const from = r && r.coordinator ? `${r.coordinator}, the coordinator` : "The coordinator";
        return h("div", { class: "msg me coord" }, h("div", {},
          h("div", { class: "bubble" }, h("span", { class: "who" }, from), parts(item)), h("div", { class: "meta num" }, clock(item.at))));
      }
      case "queued":
        return h("div", { class: "msg me queued" }, h("div", {}, h("div", { class: "bubble" }, item.text), h("div", { class: "meta" }, "Waiting for its turn")));
      case "news": return newsItem(item, sel);
      case "said": return saidItem(item);
      case "tools": return toolsItem(item);
      case "question": return questionItem(item, sel);
      case "worked": case "compacted": case "interrupted":
        return h("div", { class: "sysline" }, item.text || "");
      case "error": return h("div", { class: "errline" }, item.text || "");
      default: return null; // a kind this page doesn't know yet
    }
  }

  // ---------- the chat: drawing ----------
  function rowFor(sel) {
    return sel && rows.find((r) => isSel(r, sel));
  }

  function drawMain() {
    const sel = route();
    $("app").classList.toggle("list", !sel);
    const main = $("main");
    if (!sel) {
      main.replaceChildren(h("div", { class: "scroll" }, h("div", { class: "empty" },
        h("b", {}, "Pick a coordinator or a task"), "⌘K jumps to one by its ID, title or repository; n goes to the next that needs you.")));
      return;
    }
    const isCoord = /^c\d+$/.test(sel.id);
    const head = header(sel, rowFor(sel));
    const scroll = h("div", { class: "scroll", id: "scroll" }, h("div", { class: "col", id: "col" }));
    main.replaceChildren(head, scroll, dock(sel, isCoord));
    drawItems(true);
  }

  /** Only the header again (the snapshot changed), keeping the chat as it is. */
  function drawHead() {
    const sel = route(), old = document.querySelector(".head");
    if (sel && old) old.replaceWith(header(sel, rowFor(sel)));
  }

  function dock(sel, isCoord) {
    const to = isCoord ? `${sel.id}, ${sel.repo}'s coordinator` : `${sel.id}'s worker`;
    const placeholder = `Message ${to}`;
    return h("div", { class: "dock" }, h("div", { class: "col" },
      h("form", { class: "composer", title: LATER },
        h("textarea", { rows: "1", placeholder, "aria-label": placeholder, disabled: true }),
        h("button", { class: "send", type: "submit", "aria-label": "Send", disabled: true }, svg(ICON.up))),
      h("div", { class: "hint" }, `Typing to ${isCoord ? "a coordinator" : "a worker"} from here comes next; for now, use the board.`)));
  }

  /** The chat's items into its column; at the bottom stays at the bottom. */
  function drawItems(toEnd) {
    const col = $("col"), scroll = $("scroll"), sel = route();
    if (!col || !sel) return;
    const atEnd = toEnd || scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight < 80;
    const kids = [];
    if (!chat || chat.state === "loading") {
      kids.push(h("div", { class: "empty" }, "Reading its chat…"));
    } else if (chat.state === "missing") {
      kids.push(h("div", { class: "empty" }, h("b", {}, "Chat isn't served yet"),
        "This bullpen web doesn't serve chats yet; the sidebar still updates as things change."));
    } else if (chat.state === "error") {
      kids.push(h("div", { class: "empty" }, h("b", {}, "This chat can't be read"), chat.error));
    } else {
      if (chat.more) kids.push(h("button", { class: "btn earlier", type: "button", id: "earlier" }, "Earlier"));
      if (chat.source === "none" || !chat.items.length) {
        kids.push(h("div", { class: "empty" }, h("b", {}, "No chat to show"),
          chat.source === "none" ? "Its Claude Code transcript wasn't found: it may not have started, or it isn't Claude Code." : "Nothing has been said yet."));
      }
      chat.items.forEach((item, i) => {
        const el = itemEl(item, sel);
        if (el) kids.push(el);
        if (i === 0 && chat.source === "title" && !chat.more) {
          kids.push(h("div", { class: "source" }, "Found by its title: it started before bullpen recorded which conversation is whose."));
        }
      });
    }
    col.replaceChildren(...kids);
    if (atEnd) scroll.scrollTop = scroll.scrollHeight;
  }

  // ---------- the chat: reading it ----------
  function chatUrl(sel, before) {
    const q = new URLSearchParams();
    if (!/^c\d+$/.test(sel.id)) q.set("who", sel.who);
    if (before) q.set("before", before);
    return `/api/chat/${encodeURIComponent(sel.repo)}/${encodeURIComponent(sel.id)}?${q}`;
  }

  async function readChat(sel) {
    const key = keyOf(sel);
    chat = { key, state: "loading", items: [] };
    pending = [];
    drawItems(true);
    let res, body;
    try {
      res = await fetch(chatUrl(sel), { headers: { Accept: "application/json" } });
      body = await res.json().catch(() => null);
    } catch (e) {
      if (chat && chat.key === key) {
        Object.assign(chat, { state: "error", error: "The server didn't answer. Is bullpen web still running?" });
        drawItems(true);
      }
      return;
    }
    if (!chat || chat.key !== key) return; // another chat was picked meanwhile
    if (res.status === 404 && !(body && body.error)) chat.state = "missing";
    else if (!res.ok || !body) Object.assign(chat, { state: "error", error: (body && body.error) || `It answered ${res.status}.` });
    else {
      Object.assign(chat, { state: "ok", items: [], more: !!body.more, source: body.source, agent: body.agent });
      merge(body.items || []);
      for (const p of pending) merge(p);
    }
    pending = [];
    drawItems(true);
  }

  async function readEarlier() {
    const sel = route();
    if (!chat || chat.state !== "ok" || !chat.items.length) return;
    const key = chat.key, scroll = $("scroll"), fromEnd = scroll.scrollHeight - scroll.scrollTop;
    try {
      const res = await fetch(chatUrl(sel, chat.items[0].id), { headers: { Accept: "application/json" } });
      const body = await res.json();
      if (!res.ok || !chat || chat.key !== key) return;
      const have = new Set(chat.items.map((i) => i.id));
      chat.items = [...(body.items || []).filter((i) => !have.has(i.id) && !gone(i)), ...chat.items];
      chat.more = !!body.more;
    } catch (e) {
      return;
    }
    drawItems(false);
    scroll.scrollTop = scroll.scrollHeight - fromEnd;
  }

  const gone = (i) => i.kind === "queued" && i.gone;

  /** Items new or changed, by their ID: a changed one takes its old one's place; a
   * queued one that's gone goes. */
  function merge(items) {
    for (const item of items) {
      const at = chat.items.findIndex((i) => i.id === item.id);
      if (at < 0) {
        if (!gone(item)) chat.items.push(item);
      } else if (gone(item)) chat.items.splice(at, 1);
      else chat.items[at] = item;
    }
  }

  // ---------- the event stream ----------
  /** Open the event stream for what's shown. One that drops reconnects by itself; one
   * the server refused (a 502 while bullpen web restarts) is opened again here. Once
   * back, the chat is read again, for what it missed. */
  function watch(sel, dropped = false) {
    if (stream) stream.close();
    clearTimeout(retry);
    const w = sel ? `?watch=${encodeURIComponent(sel.repo)}/${encodeURIComponent(sel.id)}${/^c\d+$/.test(sel.id) ? "" : "/" + sel.who}` : "";
    stream = new EventSource("/events" + w);
    const key = keyOf(sel);
    const mine = stream;
    stream.onopen = () => {
      live = true;
      drawSide();
      if (dropped && sel && chat && chat.key === key) readChat(sel);
      dropped = false;
    };
    stream.onerror = () => {
      live = false;
      dropped = true;
      drawSide();
      if (mine.readyState === EventSource.CLOSED && stream === mine) retry = setTimeout(() => watch(route(), true), 3000);
    };
    stream.addEventListener("fleet", (e) => {
      live = true;
      try { setFleet(JSON.parse(e.data)); } catch (err) { /* a bad snapshot: keep the last */ }
    });
    stream.addEventListener("chat", (e) => {
      if (!chat || chat.key !== key) return;
      let data;
      try { data = JSON.parse(e.data); } catch (err) { return; }
      if (data.reset) {
        // the chat drawn again from the start (it moved to another transcript): as /api/chat has it
        Object.assign(chat, { state: "ok", items: [], more: !!data.more, source: data.source, agent: data.agent });
        pending = [];
      } else if (chat.state === "loading") return void pending.push(data.items || []);
      if (chat.state !== "ok") return;
      merge(data.items || []);
      drawItems(false);
    });
  }

  function setFleet(f) {
    fleet = f;
    rows = rowsOf(f);
    drawSide();
    drawHead();
    if (pal.on) drawPalette();
  }

  async function readFleet() {
    try {
      const res = await fetch("/api/fleet", { headers: { Accept: "application/json" } });
      if (res.ok && !fleet) setFleet(await res.json());
    } catch (e) { /* the stream brings it */ }
  }

  // ---------- ⌘K ----------
  /** The next that needs you after the one shown, as go --next takes them. */
  function nextNeeding() {
    const needs = rows.filter((r) => r.group === "needs");
    if (!needs.length) return null;
    const at = needs.findIndex((r) => isSel(r, route()));
    return needs[(at + 1) % needs.length];
  }

  function palRows() {
    const q = pal.q.trim().toLowerCase();
    const hits = rows.filter((r) => !q || `${r.id} ${r.title} ${r.repo}`.toLowerCase().includes(q));
    const out = [];
    const next = !q && nextNeeding();
    if (next) out.push({ head: "Go" }, { row: next, key: "n", text: `The next that needs you: ${next.id}${next.since ? `, waiting ${ago(next.since)}` : ""}` });
    for (const [g, name] of GROUPS) {
      const these = hits.filter((r) => r.group === g);
      if (these.length) out.push({ head: g === "done" ? "Done" : name }, ...these.map((r) => ({ row: r })));
    }
    return out;
  }

  /** Text with what matches the search underlined, as text nodes. */
  function marked(text, q) {
    if (!q) return [text];
    const out = [], low = text.toLowerCase();
    let at = 0, i;
    while (q && (i = low.indexOf(q, at)) >= 0) {
      out.push(text.slice(at, i), h("mark", {}, text.slice(i, i + q.length)));
      at = i + q.length;
    }
    out.push(text.slice(at));
    return out;
  }

  function drawPalette() {
    $("veil").classList.toggle("on", pal.on);
    if (!pal.on) return;
    const list = $("pal-list"), q = pal.q.trim().toLowerCase();
    const items = palRows();
    const picks = items.filter((x) => x.row);
    pal.i = Math.min(pal.i, Math.max(0, picks.length - 1));
    let n = 0;
    const kids = items.map((x) => {
      if (x.head) return h("div", { class: "pal-h" }, x.head);
      const r = x.row, hl = n++ === pal.i;
      return h("a", { class: hl ? "pal-row hl" : "pal-row", href: hashOf(r.repo, r.id, "worker"), "data-pick": n - 1 },
        mark(r.mark), h("span", { class: "id" }, x.key || r.id),
        h("span", { class: "tt" }, x.text ? x.text : marked(r.title, q)), h("span", { class: "rp" }, x.text ? "" : r.repo));
    });
    if (!picks.length) kids.push(h("div", { class: "pal-none" }, q ? `Nothing open or closed lately matches “${pal.q}”.` : "Nothing to jump to yet."));
    list.replaceChildren(...kids);
    const hl = list.querySelector(".hl");
    if (hl) hl.scrollIntoView({ block: "nearest" });
    pal.picks = picks;
  }

  function openPalette(on) {
    pal.on = on;
    pal.q = "";
    pal.i = 0;
    $("pal-in").value = "";
    drawPalette();
    if (on) $("pal-in").focus();
  }

  function goTo(r) {
    if (!r) return;
    openPalette(false);
    location.hash = hashOf(r.repo, r.id, "worker");
  }

  // ---------- what the person does ----------
  document.addEventListener("click", (e) => {
    const tools = e.target.closest("[data-tools] > button");
    if (tools) {
      const box = tools.parentElement, id = box.dataset.tools;
      if (opened.has(id)) opened.delete(id);
      else opened.add(id);
      box.classList.toggle("open", opened.has(id));
      tools.setAttribute("aria-expanded", String(opened.has(id)));
      return;
    }
    if (e.target.closest("#earlier")) return void readEarlier();
    if (e.target.closest("#jump")) return openPalette(true);
    if (e.target.closest("#theme")) return toggleTheme();
    if (e.target.closest(".pal-row")) return openPalette(false);
    if (e.target.id === "veil") openPalette(false);
  });

  $("pal-in").addEventListener("input", (e) => {
    pal.q = e.target.value;
    pal.i = 0;
    drawPalette();
  });

  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      return openPalette(!pal.on);
    }
    if (pal.on) {
      const picks = pal.picks || [];
      if (e.key === "Escape") openPalette(false);
      else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && picks.length) {
        e.preventDefault();
        pal.i = (pal.i + (e.key === "ArrowDown" ? 1 : -1) + picks.length) % picks.length;
        drawPalette();
      } else if (e.key === "Enter" && picks[pal.i]) {
        e.preventDefault();
        goTo(picks[pal.i].row);
      }
      return;
    }
    const typing = e.target.closest && e.target.closest("input, textarea, [contenteditable]");
    if (e.key === "n" && !typing && !e.metaKey && !e.ctrlKey && !e.altKey) goTo(nextNeeding());
  });

  function toggleTheme() {
    const root = document.documentElement;
    const now = root.dataset.theme || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
    root.dataset.theme = now === "dark" ? "light" : "dark";
    try { localStorage.setItem("theme", root.dataset.theme); } catch (e) { /* kept for this visit only */ }
  }

  function picked() {
    const sel = route();
    if (shown === undefined || shown !== keyOf(sel)) {
      shown = keyOf(sel);
      chat = null;
      drawSide();
      drawMain();
      watch(sel);
      if (sel) readChat(sel);
    }
  }

  try {
    const t = localStorage.getItem("theme");
    if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  } catch (e) { /* the system's theme */ }
  window.addEventListener("hashchange", picked);
  setInterval(drawSide, 30 * 1000); // ages move on
  picked();
  readFleet();
})();
