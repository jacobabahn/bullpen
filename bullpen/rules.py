"""The rules every worker and reviewer gets: the repo's .bullpen/rules.md, then the person's own."""

import os
import subprocess

from .core import Repo, command, git, repo_root, task_base_ref


RULES_FILE = ".bullpen/rules.md"


def personal_rules(repo):
    return repo.dir / "rules.md"


def rule_sources(repo, committed, where):
    """[(where it came from, text)]: the repo's rules file first, then the personal one."""
    out = []
    if committed and committed.strip():
        out.append((f"{RULES_FILE} {where}".strip(), committed.strip()))
    p = personal_rules(repo)
    if p.exists() and p.read_text().strip():
        out.append((str(p), p.read_text().strip()))
    return out


def committed_rules_on(repo, ref):
    """The repo's rules file as committed on ref, or None."""
    p = git(repo.root, "show", f"{ref}:{RULES_FILE}", check=False)
    return p.stdout if p.returncode == 0 else None


def rules_text(sources):
    return "\n\n".join(text for _, text in sources)


def task_rules(repo, t):
    """The rules the task's worker was given; for a task started before rules
    were saved, the ones on its base now."""
    saved = repo.dir / "briefs" / f"{t['id']}.rules.md"
    if saved.exists():
        return saved.read_text().strip()
    return rules_text(rule_sources(repo, committed_rules_on(repo, task_base_ref(repo, t)), ""))


def cmd_rules(args):
    repo = Repo(repo_root())
    if args.edit:
        p = personal_rules(repo)
        p.parent.mkdir(parents=True, exist_ok=True)
        editor = os.environ.get("VISUAL") or os.environ.get("EDITOR") or "vi"
        subprocess.call([*editor.split(), str(p)])
        return
    base = repo.base()
    ref = f"origin/{base}" if repo.has_origin() and git(
        repo.root, "rev-parse", "-q", "--verify", f"origin/{base}", check=False).returncode == 0 else base
    sources = rule_sources(repo, committed_rules_on(repo, ref), f"on {ref}")
    if not sources:
        print(f"No rules. Commit them as {RULES_FILE}, or keep your own in "
              f"{personal_rules(repo)} (`bullpen rules --edit`).")
        return
    print("\n\n".join(f"# From {where}\n\n{text}" for where, text in sources))


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "rules", cmd_rules, "Setup",
                help="print the rules every worker and reviewer gets, and where they're from")
    p.add_argument("--edit", action="store_true", help="edit your own rules for this repo in $EDITOR")
