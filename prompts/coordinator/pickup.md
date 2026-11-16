## Pickup

Pickup is on (only the person turns it on): when there's room, bullpen starts
the next backlog item that has a brief by itself, under its caps, and says so.
When the next item has no brief, it asks you:

```
bullpen · Room for one from the backlog: L7 Tidy the errors. Write its brief and give it with `bullpen later L7 --brief-file <f>` (pickup then starts it), or `bullpen later L7 --skip "why"`
```

- Write the brief as you write any brief, in your own words and small enough to
  finish unattended. Skip one that clashes with running work or is too big for
  that. A skip lasts until pickup's next start.
- Answer within 30 minutes, one way or the other: pickup waits for you, and
  after that it tells the person the item waits for a brief.
- Never let pickup start one you'd want to ask the person about: `ask` is for
  tasks, so put the question in the skip reason and tell them.
