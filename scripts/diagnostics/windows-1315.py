"""Bounded #1315 instrument; patches only disposable, exact-revision checkouts."""
import os
from pathlib import Path
import re
import subprocess
import sys

TASKQ = "8ed985189a61c2c0fb91e64f50f1562056017d2b"
COREQ = "6039b01449471dbe87258d87371ec135a05b54cf"
TEST = "incompatible_running_writer_hands_over_selftest_and_receipts"


def replace(path, old, new):
    text = path.read_text()
    assert text.count(old) == 1, (path.name, old, text.count(old))
    path.write_text(text.replace(old, new))


def prepare(candidate, coreq):
    for root, sha in [(candidate, TASKQ), (coreq, COREQ)]:
        assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == sha
        assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).strip()
    capture = coreq / "src/telemetry/capture.rs"
    # Queue residence includes scheduler delay and earlier work; it is not pure CPU scheduling time.
    replace(capture, "Record(Vec<Event>),", "Record(Vec<Event>, Instant),")
    replace(capture, "self.sender.try_send(Message::Record(events))", "self.sender.try_send(Message::Record(events, Instant::now()))")
    replace(capture, "Ok(Message::Record(events)) => {", '''Ok(Message::Record(events, queued)) => {
                            probe_timing("queue-residence", queued.elapsed());''')
    replace(capture, "Flush(mpsc::Sender<Result<(), String>>),", "Flush(mpsc::Sender<Result<(), String>>, Instant),")
    replace(capture, "Message::Flush(sender);", "Message::Flush(sender, Instant::now());")
    replace(capture, "Ok(Message::Flush(reply)) => {", '''Ok(Message::Flush(reply, queued)) => {
                            probe_timing("flush-queue", queued.elapsed());''')
    replace(capture, "state.append_us.fetch_add(elapsed, Ordering::Relaxed);", '''probe_timing("append-total", began.elapsed());
                    state.append_us.fetch_add(elapsed, Ordering::Relaxed);''')
    replace(capture, "pub fn flush(&self) -> Result<(), String> {", '''pub fn flush(&self) -> Result<(), String> {
        let _probe = ProbeFlush(Instant::now());''')
    with capture.open("a") as output:
        output.write('''
// Temporary #1315 diagnostic: no payloads or paths, no change to deadlines or results.
fn probe_timing(phase: &str, elapsed: Duration) {
    if super::store_trace() {
        let _ = writeln!(std::io::stderr(), "Probe1315: phase={} elapsed_us={}", phase, elapsed.as_micros());
    }
}
struct ProbeFlush(Instant);
impl Drop for ProbeFlush {
    fn drop(&mut self) { probe_timing("flush-total", self.0.elapsed()); }
}
''')
    # Override only this disposable build; published dependency pins remain untouched.
    with (candidate / "Cargo.toml").open("a") as output:
        output.write(f'\n[patch."https://github.com/taskqorg/coreq"]\ncoreq = {{ path = "{coreq.as_posix()}" }}\ncoreq-instrument = {{ path = "{(coreq / "instrument").as_posix()}" }}\n')
    print(f"Diagnostic baseline TaskQ={TASKQ} CoreQ={COREQ}; deadline=10s", flush=True)


def run(candidate):
    env = dict(os.environ, COREQ_TELEMETRY_STORE_TRACE="1")
    logs = Path("probe-1315-logs")
    logs.mkdir(exist_ok=True)
    failures = []
    maxima = {}
    for number in range(1, 21):
        result = subprocess.run(["cargo", "test", "--locked", "--test", "telemetry_process", TEST,
                                 "--", "--exact", "--nocapture"], cwd=candidate, env=env,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
        (logs / f"run-{number:02}.log").write_text(result.stdout)
        print(f"=== PROBE {number}/20 exit={result.returncode} ===\n{result.stdout}", flush=True)
        values = re.findall(r"Probe1315: phase=([\w-]+) elapsed_us=(\d+)", result.stdout)
        phases = {phase for phase, _ in values}
        ipc = set(re.findall(r"Telemetry IPC: phase=(\w+)", result.stdout))
        if result.returncode or not {"connect", "write", "ack"} <= ipc or not {"queue-residence", "flush-queue", "append-total", "flush-total"} <= phases:
            failures.append(number)
        for phase, value in values:
            maxima[phase] = max(maxima.get(phase, 0), int(value))
    print(f"20-run summary: maxima_us={maxima}; failed_or_missing={failures}", flush=True)
    assert not failures, failures
    assert maxima["append-total"] <= 3_000_000, "Slow append captured: inspect phase traces"
    assert maxima["flush-total"] < 10_000_000, "10s deadline not demonstrated sufficient"


if __name__ == "__main__":
    if sys.argv[1] == "prepare":
        prepare(Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve())
    elif sys.argv[1] == "run":
        run(Path(sys.argv[2]).resolve())
    else:
        raise SystemExit("expected prepare or run")
