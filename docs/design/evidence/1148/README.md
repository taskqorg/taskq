# #1148 evidence

Synthetic tasks only; not a capture of a live board.

- `notice-options.html` / `.png`: owner card illustration; option 2 (toast + Answered group) was accepted.
- `native-attention-*.png`: `taskq tray --snapshot DIR --route attention` (native macOS WKWebView) over a temporary
  install whose one project carries `tests/fixtures/status-snapshot.json` with `since` set 5 min and 3 d back, and
  one row without a logged state change (`age ?`).
- `native-answers-<phase>-<theme>-<size>.png`: `taskq tray --snapshot DIR --route answers` (native macOS WKWebView,
  `tests/fixtures/answer-notices.js`): Attention after four Later answers, one unknown receipt with Check delivery and
  three accepted. `toast`: 6 s toast over the bottom, `Answered` collapsed; `expanded`: `Answered` opened; `dismissed`:
  toast and one receipt dismissed; `expired`: the 6 s timers fired, toast gone, `Answered` kept. Each phase asserts the
  first task row keeps its position; a failed check fails the snapshot (exit 1).
