# A guided tour of {id}: {title}

You write a tour of a finished change for the person who will review it. A tour
is the change in reading order: one step per logical change, each with a note
on why it changed and what to look at. The person reads the code itself from
git beside your notes, so don't copy code into them.

## What was asked

The worker's brief:

`````markdown
{brief}
`````

## What was done

- Worktree: `{worktree}`, compared with `{base_ref}`. You are in it.
- The worker's summary:

{summary}

- Review findings, if a reviewer looked at it:

{findings}

## The change

`git diff {base_ref}...HEAD`:

`````diff
{diff}
`````

Its hunks, as file and line ranges (in the new file; in the old one for a
deleted file):

{hunks}

You can read the files and run `git diff`, `git log` and `git show` for
context. Change nothing.

## How to write it

- One step per logical change, in the order that makes the change easiest to
  understand: usually the core of it first, then what builds on it, then tests
  and docs. A step can span files, and a file can be split across steps.
- Each step has a short `title`, a `note` of two to five sentences (why it
  changed, and what to look at: a risk, an edge case, a choice the person
  should agree with), and its `hunks`: each `{{"file": "<path>", "lines":
  [start, end]}}`, with line numbers from the list above. A file with no
  line numbers (a binary one) is `{{"file": "<path>"}}`.
- Cover every hunk if you can. Any you leave out goes into a last step,
  "Other changes", so leave out only what needs no explaining.
- Plain words. Don't praise the change or restate the diff.

## Your answer

Read and think first, as long as you need. Then answer with the tour as JSON,
once, and nothing else: no code fence, no text before or after it, and no second
version. If you find a mistake while you write it, fix it before you answer;
your final message is the finished tour. It looks like this:

{{"steps": [{{"title": "...", "note": "...", "hunks": [{{"file": "...", "lines": [1, 20]}}]}}]}}
