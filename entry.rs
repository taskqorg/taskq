//! Only process bootstrap lives outside generated product instrumentation.
include!(concat!(env!("OUT_DIR"), "/instrumented/main.rs"));

fn main() {
    use std::io::Write;
    // § 8 step 5: the macOS cargo runner signs local builds before they start; no telemetry, no install writes.
    #[cfg(target_os = "macos")]
    if let [cmd, paths @ ..] = &std::env::args().skip(1).collect::<Vec<_>>()[..] && cmd == "sign-local" {
        if let Err(e) = platform::sign_local(paths) { let _ = writeln!(std::io::stderr(), "Error: signing: {e}"); std::process::exit(1) }
        return;
    }
    let install = platform::default_install();
    let maintenance=Args::parse(&std::env::args().skip(1).collect::<Vec<_>>()).is_ok_and(|a|a.pos==["profile","migrate"]);
    let _telemetry = (!maintenance).then(||telemetry::install_capture(&install)).transpose()
        .map_err(|e| {let _=writeln!(std::io::stderr(),"Telemetry: unknown ({e})");}).ok();
    if let Some(reason)=telemetry::handover() {let _=writeln!(std::io::stderr(),"Telemetry: same-store compatibility budget ({reason})");}
    application();
}
