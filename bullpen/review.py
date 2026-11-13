"""Reviews: the twin of cli/src/review.rs's patch_id and close_reviewer, for tours."""

from .core import git, run, unindex_block
from .rex import rex_close
from .agents import pane


def patch_id(wt, base_ref):
    """The stable patch-id of the branch's change against its base, or None if there's none."""
    diff = git(wt, "diff", "--binary", f"{base_ref}...HEAD").stdout
    if not diff.strip():
        return None
    out = run(["git", "-C", str(wt), "patch-id", "--stable"], input=diff).stdout.split()
    return out[0] if out else None


def close_reviewer(t):
    r = t.get("review") or {}
    if r.get("block"):
        unindex_block(r["block"])
        rex_close(*pane(t, "reviewer"))
        r["block"] = None
