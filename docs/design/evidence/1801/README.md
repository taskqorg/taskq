# Notification icon and bundle id · #1801

## Root cause (A1)

- macOS notifications showed a grey placeholder instead of the TaskQ icon.
- On the owner Mac, LaunchServices held 9 `org.taskq.tray` registrations. Two were live, iconless `TaskQ.app` copies left by worker evidence installs of 2026-10-14/15 (for example the G8 owner fixture, whose log shows `NotificationRecord app:"org.taskq.tray"` from a `target/g8-owner` install).
- `tray_app` signed every install's bundle as `org.taskq.tray`, so any temp, test or evidence install could win the identifier and its icon.
- The PM unregistered the stale copies on 2026-10-29. After that, `lsregister -dump` lists one `org.taskq.tray` path: `~/Applications/TaskQ.app`.

## Fix (A2, A3)

- Only the OS default install signs its bundle as `org.taskq.tray`. Every other install kind (temp, test, evidence, custom `TASKQ_INSTALL`) signs `org.taskq.tray.test`.
- The default install runs `lsregister -f` on its bundle after each rewrite. The tray quits for a newer release, so install and update both pass through it.
- Tests: `tray_bundle_id_per_install_kind` asserts the identifier per install kind; `app_bundle_icon_resources_preserve_signature` asserts a temp install's plist carries `org.taskq.tray.test`.

## Notification screenshot (A4)

Unknown. The worker session has no Screen Recording permission (`screencapture`: `could not create image from display`), so the real notification capture needs a native capture by the PM after this change ships.
