//! Only process bootstrap lives outside generated product instrumentation.
include!(concat!(env!("OUT_DIR"), "/instrumented/main.rs"));

fn main() {
    use std::io::Write;
    // #1393: before any thread, so `which`, the systemd unit `service install` writes and children all see ~/.local/bin
    #[cfg(target_os = "linux")]
    if let Some(path) = runtime::user_bin_path(&std::env::var_os("PATH").unwrap_or_default(), std::env::var_os("HOME").as_deref()) {
        // SAFETY: single-threaded process start
        unsafe { std::env::set_var("PATH", path) }
    }
    // #1531: Claude settings cannot unset a variable; an operator session gets these empty, which means unset.
    for k in ["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"] {
        // SAFETY: single-threaded process start
        if std::env::var_os(k).is_some_and(|v| v.is_empty()) { unsafe { std::env::remove_var(k) } }
    }
    // § 8 step 5: the macOS cargo runner signs local builds before they start; no telemetry, no install writes.
    #[cfg(target_os = "macos")]
    if let [cmd, paths @ ..] = &std::env::args().skip(1).collect::<Vec<_>>()[..] && cmd == "sign-local" {
        if let Err(e) = platform::sign_local(paths) { let _ = writeln!(std::io::stderr(), "Error: signing: {e}"); std::process::exit(1) }
        return;
    }
    let install = platform::default_install();
    let args=Args::parse(&std::env::args().skip(1).collect::<Vec<_>>());
    let maintenance=args.as_ref().is_ok_and(|a|a.pos==["profile","migrate"]);
    // #1419: the release updater runs a staged candidate's offline selftest beside the old release's live writer;
    // a slow old writer must not refuse the candidate that replaces it, so the candidate qualifies a private store.
    let staged=std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some()&&args.as_ref().is_ok_and(|a|a.pos==["selftest"]);
    let private=staged.then(||std::env::temp_dir().join(format!("taskq-selftest-{}",std::process::id())));
    // The process-global writer keeps the store open until exit, and Windows cannot remove an open file:
    // each staged selftest sweeps the stores of earlier ones whose process has exited.
    if staged && let Ok(entries)=std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            let name=entry.file_name();
            let Some(pid)=name.to_str().and_then(|n|n.strip_prefix("taskq-selftest-")).and_then(|p|p.parse::<u32>().ok()) else {continue};
            if pid!=std::process::id()&&matches!(coreq::process::identity(pid),Ok(None)) {let _=std::fs::remove_dir_all(entry.path());}
        }
    }
    let _telemetry = (!maintenance).then(||telemetry::install_capture(private.as_deref().unwrap_or(&install))).transpose()
        .map_err(|e| {let _=writeln!(std::io::stderr(),"Telemetry: unknown ({e})");}).ok();
    application();
    platform::drain_telemetry();
    // ponytail: a failed selftest exits inside application() and keeps its private store for diagnosis;
    // on Windows this removal fails while the writer is open, and the next staged selftest sweeps it.
    // #1519: the drop seals the store; a seal observed by the still-global collector queues its own checkpoint span,
    // and the writer thread then re-elects itself and recreates the store after this removal.
    if let Some(private)=private {
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),||drop(_telemetry));
        let _=std::fs::remove_dir_all(private);
    } else {drop(_telemetry);}
    // #1566: returning from main ends in the same `ExitProcess` deadlock as `platform::exit`; the drop above sealed.
    #[cfg(windows)] platform::terminate(0);
}
