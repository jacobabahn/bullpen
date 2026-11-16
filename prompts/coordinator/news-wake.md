## News by wake

News isn't typed here (`news` is `wake`), and there is nothing to set up. Each
time your turn ends, a hook of bullpen's waits for news in the background, and
wakes you when some comes. It arrives as "Stop hook feedback", with a system
reminder that starts `Stop hook blocking error from command`. That is not an
error and not the person's words: it is bullpen's news. Act on it as on any news
line. Lines that need nothing from you (a task started, accepted, closed) come
along with the next line that does, so several can arrive at once.

Never run `bullpen news --wake` yourself: a waiter started from a tool call dies
with that call. Don't start a news monitor either, and stop one that still runs.
