// Posts real mouse events to one process without moving the system cursor (#2375).
// Usage: native-input PID WINDOW bounds | click X Y | drag X0 Y0 X1 Y1 | copy X Y | hid-drag X0 Y0 X1 Y1
// `hid-drag` is the one exception: the window server runs titlebar moves, so it needs a system-level drag. It refuses unless the
// session has been idle for 30 seconds and the target window is frontmost at the start point, then restores cursor and active app.
// The caller must first prove the start pixel is opaque: a transparent pixel sends the drag to the window below (see README.md).
// `copy` clicks, reports the pasteboard text and restores every earlier item from memory; earlier contents are never logged.
import AppKit

let args = CommandLine.arguments
guard args.count >= 4, let pid = pid_t(args[1]), let window = Int64(args[2]) else { exit(2) }
let numbers = args[4...].compactMap(Double.init)

// Exported CoreGraphics call: the point inside the destination window, which AppKit reads for window-targeted events.
@_silgen_name("CGEventSetWindowLocation") func setWindowLocation(_ event: CGEvent, _ point: CGPoint)

func origin() -> CGPoint {
    let info = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]])?.first { ($0[kCGWindowNumber as String] as? Int64) == window }
    let box = info?[kCGWindowBounds as String] as? [String: CGFloat] ?? [:]
    return CGPoint(x: box["X"] ?? 0, y: box["Y"] ?? 0)
}
let windowOrigin = origin()

func post(_ type: CGEventType, _ x: Double, _ y: Double, clicks: Int64 = 1) {
    guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left) else { exit(3) }
    event.setIntegerValueField(.mouseEventClickState, value: clicks)
    event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: window)
    event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: window)
    // Raw field 51 names the destination window; without it AppKit delivers a windowless event that no view receives.
    event.setIntegerValueField(unsafeBitCast(UInt32(51), to: CGEventField.self), value: window)
    setWindowLocation(event, CGPoint(x: x - windowOrigin.x, y: y - windowOrigin.y))
    event.postToPid(pid)
    usleep(30_000)
}

func click(_ x: Double, _ y: Double) {
    post(.mouseMoved, x, y)
    post(.leftMouseDown, x, y)
    post(.leftMouseUp, x, y)
}

switch args[3] {
case "bounds":
    let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]
    let bounds = list?.first { ($0[kCGWindowNumber as String] as? Int64) == window }?[kCGWindowBounds as String] as? [String: Double] ?? [:]
    print(String(data: try! JSONSerialization.data(withJSONObject: bounds, options: .sortedKeys), encoding: .utf8)!)
case "click":
    click(numbers[0], numbers[1])
case "drag":
    let (x0, y0, x1, y1) = (numbers[0], numbers[1], numbers[2], numbers[3])
    post(.mouseMoved, x0, y0)
    post(.leftMouseDown, x0, y0)
    for step in 1...20 {
        let part = Double(step) / 20
        post(.leftMouseDragged, x0 + (x1 - x0) * part, y0 + (y1 - y0) * part)
    }
    post(.leftMouseUp, x1, y1)
case "copy":
    let board = NSPasteboard.general
    let saved: [NSPasteboardItem] = (board.pasteboardItems ?? []).map { item in
        let copy = NSPasteboardItem()
        for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
        return copy
    }
    let before = board.changeCount
    click(numbers[0], numbers[1])
    let deadline = Date().addingTimeInterval(3)
    while board.changeCount == before && Date() < deadline { usleep(50_000) }
    let after = board.changeCount
    let copied = after == before ? nil : board.string(forType: .string)
    var restored = true
    if after != before {
        board.clearContents()
        restored = saved.isEmpty || board.writeObjects(saved)
    }
    let row: [String: Any] = ["change_before": before, "change_after": after, "copied": copied ?? NSNull(), "restored": restored, "items_restored": saved.count]
    print(String(data: try! JSONSerialization.data(withJSONObject: row, options: .sortedKeys), encoding: .utf8)!)
case "hid-drag":
    let (x0, y0, x1, y1) = (numbers[0], numbers[1], numbers[2], numbers[3])
    let idle = CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: CGEventType(rawValue: ~0)!)
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    let front = list.first { row in
        guard let box = row[kCGWindowBounds as String] as? [String: CGFloat], (row[kCGWindowAlpha as String] as? Double ?? 1) > 0 else { return false }
        return CGRect(x: box["X"] ?? 0, y: box["Y"] ?? 0, width: box["Width"] ?? 0, height: box["Height"] ?? 0).contains(CGPoint(x: x0, y: y0))
    }
    let frontmost = (front?[kCGWindowNumber as String] as? Int64) == window
    var row: [String: Any] = ["idle_over_30s": idle >= 30, "target_frontmost": frontmost, "performed": false]
    if idle >= 30 && frontmost {
        let cursor = CGEvent(source: nil)?.location ?? CGPoint.zero
        let active = NSWorkspace.shared.frontmostApplication
        func system(_ type: CGEventType, _ x: Double, _ y: Double) {
            CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left)?.post(tap: .cghidEventTap)
            usleep(30_000)
        }
        system(.mouseMoved, x0, y0)
        system(.leftMouseDown, x0, y0)
        for step in 1...20 {
            let part = Double(step) / 20
            system(.leftMouseDragged, x0 + (x1 - x0) * part, y0 + (y1 - y0) * part)
        }
        system(.leftMouseUp, x1, y1)
        usleep(200_000)
        CGWarpMouseCursorPosition(cursor)
        active?.activate()
        row["performed"] = true
    }
    print(String(data: try! JSONSerialization.data(withJSONObject: row, options: .sortedKeys), encoding: .utf8)!)
default:
    exit(2)
}
