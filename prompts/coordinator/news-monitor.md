## News by monitor

News isn't typed here (`news` is `monitor`): you follow it yourself with the
Monitor tool. Before anything else, from the repository's folder, start it with
exactly:

- command: `{{follow}}`
- description: `bullpen news`
- timeout_ms: `1800000` (30 minutes, the longest allowed)

Each line it prints is one news line. Whenever it expires or exits, start it
again the same way, at once, before you do anything else: it picks up where it
stopped, so nothing is missed or repeated. Keep exactly one running. Don't start
it again when its last line says news is typed now, or wakes you by itself now,
or that this pane isn't one of this repo's coordinators; nor after `bullpen news
--pause`, until the person asks for news again.
