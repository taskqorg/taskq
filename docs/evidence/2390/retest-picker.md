# Attach picker investigation for the A4 retest

PM decision `pm-2390-retest-a4-not-waive` asked for the hung picker to be investigated as a possible defect. All times are UTC on 2026-11-04.

## Result

The hang on closing the Windows common file dialog also occurs in a separate process with no TaskQ and no WebView2 code. It needs no TaskQ fix for this proof, so no blocking child was filed. The cause was not established.

## Observations

| Time | Action | Result |
|---|---|---|
| 22:08:42.55 | Real click on Attach, Conversations in the foreground | Dialog `Open` from a WebView2 helper. Not visible at +2 s, then visible, responsive and foreground from +4 s for 20 s. A hidden dialog in the first 4 s is normal startup. |
| 22:10:20.70 | File name set with `WM_SETTEXT`, then a posted IDOK | Dialog hid at once and was hung by +3 s. Owner window re-enabled by itself. Tray stayed responsive. Dialog thread waited with static CPU for over 45 s. No chip, no stored file. |
| 22:12:10 and 22:12:20.32 | Second real click, then real keystrokes for the path and Enter | Same result: hidden, hung from +3 s, no chip. |
| 22:13:33 | Control: a plain WinForms `OpenFileDialog` in my own PowerShell process, Enter on the file | Dialog was responsive, but `ShowDialog` never returned. |
| 22:17:14 | Same control, Cancel and Esc after 46 s | `ShowDialog` never returned. |
| 22:18:06 | Same control, OK on a temp file after 45 s | `ShowDialog` never returned. |

The Application Error log for the previous 6 hours was empty. A browser control was not obtained: an isolated Chrome probe never reached the foreground, and no click landed. An unrelated `ctfmon.exe` Fail Fast dialog was on the desktop. It is an untested candidate and was left alone.

## Route used

The attachment came from a real Ctrl+V with a file on the clipboard, handled by the normal paste handler of the page. The UI offers drop, paste and pick as routes. No store, IPC or DOM event was injected. The owner clipboard held no formats before this and was cleared after it.

## Still open

Reload with the native picker open was seen once in the first run and was not retested. The page rule in `src/workspace.js` considers only composing state and the last input time. A file being chosen during that reload might be lost. This does not block A4, because the attachment is already in the composer, so it stays a possible follow-up. #2366 owns Conversations history and scrolling and has no pull request yet. It is not related to the picker.
