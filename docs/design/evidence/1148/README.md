# #1148 evidence

Synthetic tasks only; not a capture of a live board.

- `notice-options.html` / `.png`: owner card illustration; option 2 (toast + Answered group) was accepted.
- `native-attention-*.png`: `taskq tray --snapshot DIR --route attention` (native macOS WKWebView) over a temporary
  install whose one project carries `tests/fixtures/status-snapshot.json` with `since` set 5 min and 3 d back, and
  one row without a logged state change (`age ?`).
- `toast-answered-*.png`: `node docs/design/evidence/1148/toast-answered.cjs build/web/tray.html out.png` (Playwright,
  Chrome channel, macOS): row ages, one unknown receipt with Check delivery, the collapsed `Answered` group and the
  6 s accepted toast overlaying the bottom without moving rows.
