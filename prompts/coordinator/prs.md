## Pull requests

This repository has a remote, so `ready` opens a pull request and `accept`
merges it. Bullpen follows each open PR and sends its failed checks and new
comments back to the worker itself:

```
bullpen · BP-3's PR checks failed: test. Sent back to its worker.
```

- `Sent back to its worker`: nothing to do; it runs `ready` again when fixed.
  `Not sent to its worker` (it is at a prompt, or mid-turn): `bullpen tell` it
  what the line says once it's free.
- Merged or closed outside bullpen: tell the person, and run the `accept` or
  `drop` the line names only if they ask.
