# Compact View window — #1730

R13: the accepted MP11/P-W4 amendment precedes the implementation. Source: `5bcfaee`; baseline: `e8499767356fbeb7f98db71cae1c0f64fec6afb1`.

R12 / § 7: `node tests/task-dashboard.browser.cjs` failed before the fix at the new A2 assertion: three saved views, eight projects and eight states must fit at 480×600 without scrolling. It passes after the fix. The browser instrument uses invented projects and in-memory IPC; it does not edit the owner's saved views.

- A1: root Product requirement and MP11/P-W4 describe wrapping grids and separate exclusive-name/checkbox actions.
- A2: browser geometry checks three views and both nine-control grids together without scrolling; naming plus Undo also fits. [Compact View](compact-view.png), [naming and Undo](naming-undo.png).
- A3: browser interactions check exclusive selection, repeated exclusive selection, adding/removing membership, exclusion from All, last removal and independent All controls.
- A4: browser interactions check Create, Cancel/Escape draft preservation, Delete/Undo, protected built-ins and existing Save/Reset/restart behavior. Storage schema and sorting are unchanged.
- A5: browser checks fixed header/footer, long project/view/deleted-view names at 20 px, horizontal overflow, icons, keyboard reachability and checkbox focus after repaint. Overflow stays in the content region.
- A6: canonical gate outcome is recorded in the PR and TaskQ result. Native installed Mac screenshots and owner visual acceptance remain **Not tested** on this Windows worker. README captures use the existing isolated fixture and manifest workflow; they do not qualify installed Mac behavior.

Images: Chromium headless shell 156.0.8078.4 on Windows, dark theme, 16 px text, 480×600 CSS pixels at scale 1. These are browser evidence, not native platform acceptance. The existing browser instrument also retains its narrow/short, light/dark and large-text checks.
