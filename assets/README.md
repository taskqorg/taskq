# App icon resources

`TaskQ.icns` and `TaskQ.ico` are the existing compact Q from `src/icon.rs`, rendered directly at each native size. They use the light-background palette and the admitting state, matching the original macOS bundle icon. They are static app identity resources; Dock and window icons follow the current tray view in memory.

Regenerate on macOS or Windows with `TASKQ_WRITE_APP_ICONS=1 cargo test --locked app_icon_assets` (PowerShell: set the environment variable first). The same test without that variable checks the committed resources against the renderer. Do not edit the binary assets by hand.

ICNS: 16, 32, 64, 128, 256, 512, 1024 px. ICO: 16, 24, 32, 48, 64, 128, 256 px. `build.rs` embeds `taskq.rc` in Windows builds; bundle creation copies the ICNS before signing under `org.taskq.tray`.
