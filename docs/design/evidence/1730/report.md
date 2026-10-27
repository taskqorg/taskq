# Compact View window — #1730

R13: the accepted MP11/P-W4 amendment precedes the implementation. Source: `5bcfaee`; baseline: `e8499767356fbeb7f98db71cae1c0f64fec6afb1`.

R12 / § 7: `node tests/task-dashboard.browser.cjs` failed before the fix at the new A2 assertion: three saved views, eight projects and eight states must fit at 480×600 without scrolling. It passes after the fix. The browser instrument uses invented projects and in-memory IPC; it does not edit the owner's saved views.

- A1 **Pass (source)**: root Product wrapping requirement and MP11/P-W4 describe wrapping grids and separate exclusive-name/checkbox actions.
- A2 **Pass (browser)**: geometry checks three views and both nine-control grids together without scrolling; naming plus Undo also fits. [Compact View](compact-view.png), [naming and Undo](naming-undo.png).
- A3 **Pass (browser)**: interactions check exclusive selection, repeated exclusive selection, adding/removing membership, exclusion from All, last removal and independent All controls.
- A4 **Pass (browser)**: interactions check Create, Cancel/Escape draft preservation, Delete/Undo, protected built-ins and existing Save/Reset/restart behavior. Storage schema and sorting are unchanged.
- A5 **Pass (browser)**: checks fixed header/footer, long project/view/deleted-view names at 20 px, horizontal overflow, icons, keyboard reachability and checkbox focus after repaint. Overflow stays in the content region.
- A6 **Fail (local gate); Not tested (installed Mac)**: the canonical gate has worker-blocked host/native checks below. Native installed Mac screenshots and owner visual acceptance remain unverified on this Windows worker. README captures use the existing isolated fixture and manifest workflow; they do not qualify installed Mac behavior.

The first canonical run found the root contract above R8's 60 KiB cap; the shortened Product wording passes at 61,440 bytes, with the accepted detail in MP11/P-W4. The second run (`python3 scripts/check.py --base e8499767356fbeb7f98db71cae1c0f64fec6afb1`) reports 530 Rust tests passed, 18 failed, 36 ignored in 77.172 s. The failures are host-policy/lifecycle/native fixture checks under the inherited worker environment, including explicit worker-authority refusals. These are blocked local checks, not passes; merge-result CI remains required under R12 and § 7. No worker identity or authority guard was removed.

The gate stops at those Rust failures. Its remaining checks were run separately: all 25 Node component scripts, screenshot/release script checks, the affected browser instrument and isolated offline selftest pass (99 golden traces; board unknown). The workspace source-hash check failed before the intended manifest refresh and passes after it; bundle/release byte protection remains enforced. Windows README captures use binary SHA-256 `acaa2bd32fb227506899f82885fbcec6122e3903e6eaa3df3e59725d89812cc6`; the shared Cargo target required filling that digest from the actual captured binary, and every image digest was verified.

Images: Chromium headless shell 156.0.8078.4 on Windows, dark theme, 16 px text, 480×600 CSS pixels at scale 1. These are browser evidence, not native platform acceptance. The existing browser instrument also retains its narrow/short, light/dark and large-text checks.

## Requeue verification

Rebased onto `e83d64fac056e973c0db9b5ab84a5f8c7064e8e8` after PR #1732 conflicted. Kept main's CSS changes and recalculated workspace source hashes. Restored the explicit A5 assertion that exactly one content container scrolls in the 20 pt long-name case; it passes with the compact-grid and scope/draft checks. This retains nested-scroll regression protection without changing accepted behavior (R12; § 7). Native installed Mac evidence is still unavailable on this Windows attempt and requires a Mac-host task before A6 is complete.
