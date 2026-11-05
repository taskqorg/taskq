//! DashQ connection commands: enroll and connection status (docs/design/enrollment.md).

use crate::cli::Args;
use crate::cli::service_cmd::service_id;
use crate::config::Ctx;
use crate::paths::dot;
use crate::{Res, data, enrollment, snapshot, telemetry_sync};
use serde_json::Value;

/// `enroll`: pairs with DashQ for observation only; no task answers or commands, and the board stays the truth.
pub(crate) fn enroll_cmd(c: &Ctx, a: &Args) -> Res<()> {
    let (b, cfg) = (c.board.as_ref(), &c.cfg);
    let url = a.pos.get(1).ok_or("enroll needs the DashQ https URL")?;
    let board = enrollment::BoardScope::new(cfg.board.name, &cfg.web_host, &cfg.repo)?;
    let dest = enrollment::destination(url)?;
    if let Some(old) = enrollment::active(&cfg.install)?.filter(|o| {
        o["destination"] != dest.as_str() || enrollment::approved(o).as_ref() != Some(&board)
    }) {
        println!(
            "switch: {} {} -> {dest} {}",
            old["destination"].as_str().unwrap_or("?"),
            enrollment::approved(&old).map_or("unknown".into(), |b| b.value()),
            board.value()
        );
    }
    println!("board scope: {} {}", enrollment::SCHEMA, board.value());
    println!("grant: observation only; no task answers or commands");
    let telemetry = a.flags.contains_key("telemetry");
    if telemetry {
        println!(
            "telemetry: {} (local telemetry rows; revoke with taskq enroll without --telemetry or taskq disconnect)",
            telemetry_sync::SCHEMA
        );
    }
    let conn = enrollment::pair(
        &cfg.install,
        url,
        &cfg.me,
        &board,
        !a.flags.contains_key("headless"),
        telemetry,
    )
    .map_err(|e| {
        format!("pairing: {e}\nNext: rerun taskq enroll {url} and approve the new code")
    })?;
    // the CLI runs no runtime probe: login reads unknown until the service's next send
    let local = enrollment::Local {
        host: cfg.me.clone(),
        service: enrollment::installation_id(&cfg.install)
            .ok()
            .map(|i| service_id(&i)),
        ttl: cfg.ttl.max(0) as u64,
        agents: Value::Null,
        progress: Default::default(),
    };
    let (_, rec) = enrollment::record(
        &cfg.install,
        &snapshot::read(b, cfg, &c.raw, &[]),
        &conn,
        &local,
    )?;
    match enrollment::first_receipt(&cfg.install, &rec, 15)? {
        "projected" => println!(
            "connected: {dest} workspace {} generation {}; first snapshot projected",
            conn["workspace"].as_str().unwrap_or("?"),
            conn["generation"]
        ),
        s => {
            return Err(format!(
                "approved, first snapshot {s}, not yet projected\nNext: taskq connection status"
            ));
        }
    }
    Ok(())
}

/// `connection status`: reports the pairing and the service's last delivery.
pub(crate) fn connection_status(c: &Ctx) -> Res<()> {
    let (root, cfg) = (c.cfg.root.as_path(), &c.cfg);
    let st = enrollment::state(&cfg.install)?;
    let st = st.as_ref().unwrap_or(&Value::Null);
    let (act, pend) = (&st["active"], &st["pending"]);
    match (act.is_object(), pend.is_object()) {
        (true, _) => println!("connection: active"),
        (false, true) => println!("connection: pending approval"),
        _ => println!("connection: none"),
    }
    let c = if act.is_object() { act } else { pend };
    if c.is_object() {
        println!("destination: {}", c["destination"].as_str().unwrap_or("?"));
        println!(
            "board scope: {} {}",
            c["scope"]["name"].as_str().unwrap_or("none"),
            c["scope"]["value"].as_str().unwrap_or("")
        );
    }
    if act.is_object() {
        println!(
            "workspace: {}  generation: {}",
            act["workspace"].as_str().unwrap_or("?"),
            act["generation"]
        );
        println!("grant: observation only; no task answers or commands");
    }
    if let Some(id) = st["installation"].as_str() {
        println!("installation: {id}");
    }
    let n = st["revoke"].as_array().map_or(0, Vec::len);
    if n > 0 {
        println!("revokes pending: {n}");
    }
    let local = dot(root).join("status.json");
    let report = data::json(&local)
        .ok()
        .flatten()
        .filter(|_| snapshot::status_live(&local, cfg.ttl));
    match report.as_ref().map(|r| &r["connection"]) {
        Some(Value::String(o)) => println!("delivery: {o}"),
        Some(_) => println!("delivery: unknown (the service has not tried yet)"),
        None => println!("delivery: unknown (no fresh service report for this project)"),
    }
    if act.is_object() {
        println!(
            "telemetry: {}",
            telemetry_line(act, report.as_ref().map(|r| &r["telemetry_sync"]))
        );
    }
    Ok(())
}

/// `taskq connection status` telemetry line (docs/design/standalone-telemetry.md#status-a5); unknown stays unknown.
fn telemetry_line(active: &Value, report: Option<&Value>) -> String {
    if !telemetry_sync::granted(active) {
        return format!(
            "off (grant lacks {}; taskq enroll <url> --telemetry)",
            telemetry_sync::SCHEMA
        );
    }
    let Some(r) = report.filter(|r| r["state"].is_string()) else {
        return "unknown (no fresh service report for this project)".into();
    };
    let n = |k: &str| r[k].as_u64().map_or("unknown".into(), |v| v.to_string());
    let mut out = r["state"].as_str().unwrap_or("unknown").to_string();
    if let Some(e) = r["reason"].as_str().or(r["last_error"].as_str()) {
        out += &format!(" ({e})")
    }
    if r["state"] != "off" {
        out += &format!(
            "; last synced {}; backlog {} rows, {} bytes; gap {} rows",
            r["last_synced"]
                .as_u64()
                .map_or("never".into(), |t| t.to_string()),
            n("backlog_rows"),
            n("backlog_bytes"),
            n("gap_rows")
        );
    }
    out
}
