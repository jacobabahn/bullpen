"""A task's review, as `show` says it (bullpen/review.py's review_kind)."""

from bullpen.json import Doc


def review_kind(d: Doc, r: Int) -> String:
    """ "full review", "light review (docs only)", "follow-up review", for review r in d."""
    var kind = d.str(r, "kind")
    if not kind:
        return "review"
    var word: String
    if kind == "full":
        word = "full review"
    elif kind == "light":
        word = "light review"
    elif kind == "followup":
        word = "follow-up review"
    elif kind == "tour":
        word = "review of the person's comments"
    elif kind == "rebase":
        word = "rebase check"
    else:
        word = kind  # Python raises a KeyError here
    return word + (" (" + d.str(r, "why") + ")" if d.truthy(r, "why") else "")
