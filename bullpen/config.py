"""`config`: showing and changing settings, the repo's and the global ones."""

import json
import os
import re
import shlex
import sys

from .core import (
    DEFAULT_CONFIG, Fail, INTERNAL, REPO_ONLY, Repo, SETTINGS, STATE, command, config_source,
    global_config_path, home, homes, inherited_config, model_phrase, read_global,
    read_global_file, read_json, reindex, repo_root, write_json,
)
from .coordinators import alive, calling_coordinator, coordinator, coordinator_at
from .news import NEWS_MARK, follow_cmd, news_mode_line, news_size, set_news_cursor, type_news
from .task_line import term_width


# The flags `config` takes that are per repository, for messages: the repo-only
# settings', and --board-others with its --coordinator (per board).
PER_REPO_FLAGS = tuple(s.name for s in SETTINGS if s.repo_only and s.kind != "file") + \
    ("board_others", "coordinator")


def config_flags(p):
    """config's flag for each setting that has one, from SETTINGS."""
    for s in SETTINGS:
        if s.kind == "file":
            continue
        if s.kind == "choice":
            p.add_argument(s.flag, choices=s.choices, help=s.full_help())
        elif s.kind == "on/off":
            p.add_argument(s.flag, choices=["on", "off"], help=s.full_help())
        elif s.kind == "number":
            p.add_argument(s.flag, type=int, metavar=s.metavar, help=s.full_help())
        else:  # text, model
            p.add_argument(s.flag, metavar=s.metavar or ("MODEL" if s.kind == "model" else None),
                           help=s.full_help())


def setting_value(s, v):
    """The value config's flag gives setting s, checked."""
    if s.kind == "on/off":
        return v == "on"
    if s.kind == "number" and v < s.least:
        raise Fail(f"{s.flag} takes {s.says}")
    if s.pattern and not re.fullmatch(s.pattern, v):
        raise Fail(f"{s.name} must be {s.says}")
    if s.clears:
        return v.strip() or None
    return v


def config_changes(args):
    """{key: value} for the settings config's flags set, each checked."""
    ch = {}
    for s in SETTINGS:
        v = getattr(args, s.name, None) if s.kind != "file" else None
        if v is not None and (v != "" or s.clears):
            ch[s.name] = setting_value(s, v)
    if args.model:  # --model is each role's, unless its own flag says otherwise
        for key in ("worker_model", "reviewer_model", "reviewer_followup_model"):
            ch.setdefault(key, args.model)  # "default" too: it drops the command's own --model
    return ch


def check_unset(keys, repo=None):
    """The keys --unset names, checked: settings bullpen has (or this repo's config.json
    has), and not one only a repository has, unless the repo's own is just switched
    off (base, push_remote)."""
    droppable = [k for k in DEFAULT_CONFIG if k not in REPO_ONLY or (repo and DEFAULT_CONFIG[k] is None)]
    for k in keys:
        if k in REPO_ONLY and k not in droppable:
            raise Fail(f"{k} is this repository's own: " + (
                "it can't be in the global config.json" if not repo else f"change it with --{k.replace('_', '-')}"))
        if k in INTERNAL or (k not in DEFAULT_CONFIG and not (repo and k in repo.own)
                             and k not in read_global_file()):
            raise Fail(f"no setting {k}: it is one of " + ", ".join(
                k for k in DEFAULT_CONFIG if k not in REPO_ONLY or DEFAULT_CONFIG[k] is None))
    return keys


def cmd_config(args):
    if args.global_:
        return config_global(args)
    repo = Repo(repo_root())
    ch = config_changes(args)
    for k in check_unset(args.unset or [], repo):
        repo.unset(k)
    if "news" in ch and ch["news"] == "monitor" and repo.cfg.get("news") != "monitor":
        for cid in [None] + [c["id"] for c in repo.cfg["coordinators"]]:
            set_news_cursor(repo, news_size(repo), cid)  # what was typed already isn't news again
        me = calling_coordinator(repo)
        told = [c["id"] for c in repo.cfg["coordinators"] if c is not me and alive(c)]
        if told:
            # The last typed line: running coordinators have to hear of the switch.
            type_news(repo, NEWS_MARK + "news now comes by monitor and is no longer typed: "
                      f"start the Monitor tool on `{follow_cmd()}` now, as your guide says", told)
    if args.board_others:
        c = coordinator(repo, args.coordinator) if args.coordinator else \
            coordinator_at(repo, os.environ.get("REX_BLOCK"))
        if not c:
            raise Fail(f"no coordinator {args.coordinator}" if args.coordinator else
                       "--board-others is per board: run it in a coordinator's pane or board, or add --coordinator ID")
        c["board_others"] = args.board_others == "on"
    for k, v in ch.items():
        repo.set(k, v)
    if "news" in ch:
        print(news_mode_line(repo), file=sys.stderr)
    repo.save()
    reindex(repo)
    show_config(repo, args.json)


def config_global(args):
    """`config --global`: change the global config.json, which every repository's
    settings fall back on."""
    per_repo = [f"--{f.replace('_', '-')}" for f in PER_REPO_FLAGS if getattr(args, f) is not None
                and getattr(args, f) is not False]
    if per_repo:
        raise Fail(f"{', '.join(per_repo)} {'is' if len(per_repo) == 1 else 'are'} per repository, "
                   "so not for the global config: run it without --global, in that repository")
    ch = config_changes(args)
    keys = check_unset(args.unset or [])
    path = global_config_path()
    g = read_global_file()
    if ch or keys:
        for k in keys:
            g.pop(k, None)
        g.update(ch)
        write_json(path, g)
        for cfg in (STATE / "repos").glob("*/config.json"):
            root = (read_json(cfg, {}) or {}).get("root") if cfg.exists() else None
            if root:
                reindex(Repo(root))  # stuck_after can come from here
    try:
        repo = Repo(repo_root())
    except Fail:
        repo = None
    show_config(repo, args.json)


def config_value(v):
    if isinstance(v, list) and all(isinstance(a, str) for a in v):
        return shlex.join(v)
    return v if isinstance(v, str) else json.dumps(v)


# The groups `config` lists the settings in, in SETTINGS's order; a setting not in
# one goes under "Other".
CONFIG_GROUPS = [(g, [s.name for s in SETTINGS if s.group == g])
                 for g in dict.fromkeys(s.group for s in SETTINGS if s.group)]


def config_groups(keys):
    """[(title, keys)]: keys in CONFIG_GROUPS's groups and order, the rest under Other."""
    named = {k for _, ks in CONFIG_GROUPS for k in ks}
    out = [(title, [k for k in ks if k in keys]) for title, ks in CONFIG_GROUPS]
    out.append(("Other", [k for k in keys if k not in named]))
    return [(title, ks) for title, ks in out if ks]


def show_config(repo, as_json=False):
    """Each setting, with where its value comes from: this repo's config.json ("repo"),
    the global one ("global") or bullpen's own default ("default"). Without a repo,
    what a repository gets when it sets nothing itself."""
    path = global_config_path()
    if repo:
        cfg = repo.cfg
        sources = {k: config_source(repo, k) for k in cfg if k not in INTERNAL}
    else:
        glob = read_global()
        cfg = {k: v for k, v in inherited_config().items() if k not in REPO_ONLY}
        sources = {k: "global" if k in glob else "default" for k in cfg}
    models = {role: model_phrase(cfg[role], cfg.get(f"{role}_model")) for role in ("worker", "reviewer")}
    if as_json:
        print(json.dumps({**({"repo": str(repo.root), "state": str(repo.dir)} if repo else {}), **cfg,
                          "models": models, "global": str(path), "sources": sources}, indent=2))
        return
    head = [("repo", home(repo.root)), ("state", home(repo.dir / "config.json"))] if repo else \
        [("repo", "none: not in a git repository")]
    head.append(("global", home(path) + ("" if path.exists() else " (none yet)")))
    for k, v in head:
        print(f"{k + ':':<8}{v}")
    keys = [k for k in sources if k not in ("next", "next_coordinator", "coordinators")]
    cols = term_width()
    for title, group in config_groups(keys):
        print(f"\n{title}")
        width = max(map(len, group))
        vals = {k: homes(str(config_value(cfg[k]))) for k in group}
        # on a terminal, a value too long to fit beside the others isn't lined up with them
        room = cols - width - 6 - max(len(sources[k]) for k in group) if cols else None
        vwidth = max((len(v) for v in vals.values() if room is None or len(v) <= room), default=0)
        for k in group:
            print(f"  {k:<{width}}  {vals[k]:<{vwidth}}  {sources[k]}")
    print()
    print(f"models: worker {models['worker']}; reviewer {models['reviewer']}")
    if repo:
        cs = ", ".join(f"{c['id']} ({c.get('block')})" for c in cfg["coordinators"])
        print(f"coordinators: {cs or 'none: `bullpen up` starts one'}")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "config", cmd_config, "Setup",
                help="show or change this repo's settings (--global: every repo's)")
    p.add_argument("--global", dest="global_", action="store_true",
                   help="change the settings every repository gets unless it sets its own, in "
                        "$XDG_CONFIG_HOME/bullpen/config.json (default ~/.config/bullpen/config.json)")
    p.add_argument("--unset", action="append", metavar="KEY",
                   help="drop this repo's own value for a setting, like merge or worker_model, so the "
                        "global one (or the default) applies; with --global, drop the global one (repeatable)")
    p.add_argument("--json", action="store_true",
                   help="print the settings as JSON, with each one's source under \"sources\"")
    config_flags(p)
    p.add_argument("--model", help="claude's --model for workers and reviewers, follow-up ones too "
                   "(an alias like opus, or a model ID; \"default\": let Claude Code pick)")
    p.add_argument("--board-others", choices=["on", "off"],
                   help="with several coordinators: this coordinator's board shows the other ones' tasks too "
                        "(default off; the board's o key)")
    p.add_argument("--coordinator", metavar="ID",
                   help="the coordinator --board-others is for (default: the one whose pane or board runs it)")
