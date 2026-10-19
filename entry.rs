//! Only process bootstrap lives outside generated product instrumentation.
include!(concat!(env!("OUT_DIR"), "/instrumented/main.rs"));

fn main() {
    use std::io::Write;
    let install = platform::default_install();
    let maintenance=Args::parse(&std::env::args().skip(1).collect::<Vec<_>>()).is_ok_and(|a|a.pos==["profile","migrate"]);
    let _telemetry = (!maintenance).then(||telemetry::install_capture(&install)).transpose()
        .map_err(|e| {let _=writeln!(std::io::stderr(),"Telemetry: unknown ({e})");}).ok();
    if let Some(reason)=telemetry::handover() {let _=writeln!(std::io::stderr(),"Telemetry: handover store ({reason})");}
    application();
}
