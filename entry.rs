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
    let _telemetry = (!maintenance).then(||telemetry::install_capture(private.as_deref().unwrap_or(&install))).transpose()
        .map_err(|e| {let _=writeln!(std::io::stderr(),"Telemetry: unknown ({e})");}).ok();
    if let Some(reason)=telemetry::handover() {let _=writeln!(std::io::stderr(),"Telemetry: same-store compatibility budget ({reason})");}
    application();
    // ponytail: a failed selftest exits inside application() and keeps its private store for diagnosis.
    if let Some(private)=private {drop(_telemetry);let _=std::fs::remove_dir_all(private);}
}
