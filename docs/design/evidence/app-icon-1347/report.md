# App icon qualification (#1347)

The existing Q renderer supplies app resources and live app/window icons. The owner retained its state logic and requested centering the square body independently of the descending tail (R13, section 4/M1).

Before: `compact` placed an 18 pt body at (2, 0.2) in a 22 pt canvas, lifting its center by 1.8 pt. The retained `app-compact-run-light-256.png` crop shows this offset. After: the body has equal horizontal and vertical insets; its scale leaves room for the whole tail. The paused Summary overlay uses the same body-centering rule. State selection is unchanged.

Checks:
- `cargo test --locked --bin taskq q_body_is_centered`: passed at 16, 22, 44 and 256 px for play/pause. This new regression check protects body centering; no existing assertions were weakened.
- `cargo test --locked --bin taskq app_bundle_icon_resources_preserve_signature`: passed. The packaged ICNS contains 16–1024 px images, the bundle retains `org.taskq.tray`, and `codesign --verify --strict` succeeds.
- `cargo run --locked -- tray --snapshot <temporary-directory> --route icon`: renderer evidence; isolated fixture install. The 256 px running Q was visually inspected with the tail fully visible.

A2's accepted amendment replaces the TCC screenshot with signed bundle identity/resource verification; no live TCC claim. Finder, Dock, Cmd-Tab and notification screenshots remain unqualified for this candidate: Computer Use inventory failed with `codex app-server exited before returning a response`. Windows native appearance and toast rendering are not observed on this Mac. The owner still approves the visual result before merge (A4); generated screenshots alone are not approval (R12).

After crops: [play](after-app-compact-run-light-256.png), [pause](after-app-compact-pause-dark-256.png), [16 px](after-app-compact-run-light-16.png), [tray](after-icon-compact-run-dark-34pt@2x.png). Play, pause, 16 px and the Summary pause overlay were visually inspected.

Canonical gate: `cargo test --locked` failed (401 passed, 16 failed, 34 ignored). Host install/policy/lifecycle fixtures encounter worker guards (`a worker cannot install the host service`, `a worker cannot change host policy`, `a worker cannot restart the host service or request Stop`): sandbox-blocked, not PASS. The project-overlay fixture also reported `unsupported` instead of `invalid`; its cause remains unknown. No guards or failing assertions were changed. Acceptance still needs green merge-result CI and the remaining native evidence.
