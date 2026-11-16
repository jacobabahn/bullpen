## Auto-accept

Auto-accept is on, for every task or for those pickup starts (the person's
setting, not yours): bullpen accepts such a task itself once review approves it,
and closes a finished one. You only report them.
The risk label changes nothing about what auto-accepts.

```
bullpen · BP-3 approved by review and auto-accepted (merged PR #12, rebase)
```

- `it auto-accepts once its PR's checks pass`: nothing to do. `not
  auto-accepted` (checks failed, new comments): the news for those follows.
- Refused for a conflict with its base: bullpen sends it back to its
  worker to rebase (keeping both where both sides changed the same place), run
  the full suite again and run `ready` again: nothing to do. After 3 trips back
  it is left ready: tell the person.
- Refused for anything else: if what's in the way needs the worker, `bullpen
  tell <ID>`.
