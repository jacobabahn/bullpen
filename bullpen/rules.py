"""The rules every worker and reviewer gets: the repo's .bullpen/rules.md, then the person's own.
A twin of cli/src/rules.rs (with the `rules` command) for review.py's task_rules."""

from .core import git
from .modes import task_base_ref


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
