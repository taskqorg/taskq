// Read-only native sampler for #2375: window bounds of one process, plus main and WebContent CPU.
// Polls CGWindowList (no Screen Recording needed for bounds) and proc_pid_rusage. It sets nothing on the target.
// Usage: window-sampler PID SECONDS HZ [TITLE] [EVENTS]   Output: JSON lines on stdout.
// EVENTS is the instrument log: its first "web_pid" names the WebContent process of a target macOS attributes to its launcher.
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 4, let pid = Int32(args[1]), let seconds = Double(args[2]), let hz = Double(args[3]) else {
    fputs("usage: window-sampler PID SECONDS HZ [TITLE]\n", stderr); exit(2)
}
let title = args.count > 4 ? args[4] : "TaskQ"
let eventsPath = args.count > 5 ? args[5] : nil
var timebase = mach_timebase_info_data_t(); mach_timebase_info(&timebase)
let tickNanos = Double(timebase.numer) / Double(timebase.denom)
typealias Responsible = @convention(c) (Int32) -> Int32
let responsible = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid")
    .map { unsafeBitCast($0, to: Responsible.self) }

func cpuSeconds(_ p: Int32) -> Double? {
    var info = rusage_info_v4()
    let r = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(p, RUSAGE_INFO_V4, $0) }
    }
    return r == 0 ? Double(info.ri_user_time + info.ri_system_time) * tickNanos / 1e9 : nil
}
// A WebContent process belongs to the target when macOS names the target as its responsible process.
func webContent() -> [Int32] {
    var pids = [Int32](repeating: 0, count: 4096)
    let n = proc_listallpids(&pids, Int32(pids.count * 4))
    guard n > 0, let responsible else { return [] }
    var name = [CChar](repeating: 0, count: 256)
    return pids[0..<Int(n)].filter {
        proc_name($0, &name, 256) > 0 && String(cString: name) == "com.apple.WebKit.WebContent" && responsible($0) == pid
    }
}
func logged() -> [Int32] {
    guard let eventsPath, let text = try? String(contentsOfFile: eventsPath, encoding: .utf8), let r = text.range(of: "\"web_pid\":") else { return [] }
    return Int32(text[r.upperBound...].prefix { $0.isNumber }).map { [$0] } ?? []
}
func emit(_ o: [String: Any]) {
    let d = try! JSONSerialization.data(withJSONObject: o, options: [.sortedKeys])
    FileHandle.standardOutput.write(d); FileHandle.standardOutput.write(Data("\n".utf8))
}
var cpu: [Int32: Double] = [:]
var last: [Int: [Double]] = [:]
let start = Date()
var samples = 0, changes = 0, nextCpu = 0.0
let epoch = { Date().timeIntervalSince1970 }
while true {
    let t = Date().timeIntervalSince(start)
    if t >= seconds || kill(pid, 0) != 0 { break }
    let list = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
    for w in list where (w["kCGWindowOwnerPID"] as? Int32) == pid && (w["kCGWindowName"] as? String) == title {
        guard let id = w["kCGWindowNumber"] as? Int, let b = w["kCGWindowBounds"] as? [String: Double] else { continue }
        let v = [b["X"] ?? 0, b["Y"] ?? 0, b["Width"] ?? 0, b["Height"] ?? 0]
        if last[id] != v {
            if last[id] != nil { changes += 1 }
            last[id] = v
            emit(["kind": "bounds", "epoch": epoch(), "window": id, "x": v[0], "y": v[1], "w": v[2], "h": v[3], "bottom": v[1] + v[3],
                  "onscreen": (w["kCGWindowIsOnscreen"] as? Int) ?? 0, "alpha": (w["kCGWindowAlpha"] as? Double) ?? -1])
        }
    }
    samples += 1
    if t >= nextCpu {
        for p in webContent() + logged() { cpu[p] = cpuSeconds(p) ?? cpu[p] }
        // Cumulative seconds: a reader subtracts any two rows to get CPU over an exact interval.
        emit(["kind": "cpu", "epoch": epoch(), "main_s": cpuSeconds(pid) as Any, "web_s": Dictionary(uniqueKeysWithValues: cpu.map { (String($0.key), $0.value) })])
        nextCpu += 1
    }
    Thread.sleep(forTimeInterval: 1 / hz)
}
emit(["kind": "end", "epoch": epoch(), "samples": samples, "bounds_changes": changes])
