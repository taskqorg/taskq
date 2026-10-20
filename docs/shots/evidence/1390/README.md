# Tray ask card: worker star and PM pick

`tray-ask-star-only.png`: the card without a pick shows only the worker's ★.
`tray-ask-pm-pick.png`: the same card with `taskq pick 841.2 --text "no client uses the old API since v3"` shows `PM 841.2: <reason>` under the choices; both choices stay the owner's.

From the repository root, after `npm ci` and a build (`build/web/tray.html`), with Playwright installed:

```sh
node docs/shots/evidence/1390/capture.cjs docs/shots/evidence/1390
```

The capture renders the bundled tray HTML in headless Chromium with the `pm pick beside the worker star` row of `tests/fixtures/answer-card.json`; it is not a native WebView capture.
