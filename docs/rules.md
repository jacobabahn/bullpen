# Rules

Rules every worker and reviewer should follow go in `.bullpen/rules.md`, committed
in the repository, so everyone using bullpen on it gets them. Rules of your own go
in `rules.md` in bullpen's folder for the repository (`bullpen rules --edit` opens
it in `$EDITOR`); they aren't committed. `start` puts both, the repository's first,
in the worker's brief under "Rules for this repository", and the reviewer gets the
same rules and checks the work against them. No rules, no heading.

The repository's file is read as committed on the task's base branch (`origin/<base>`,
or the local base branch without a remote): an uncommitted edit in your checkout, or
a task's own edit when it is started again, isn't used. `bullpen rules`
prints the rules in effect and where each came from. A coordinator that `bullpen up`
starts is told to run it first, so it knows what its briefs needn't repeat.
