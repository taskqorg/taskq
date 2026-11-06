use super::*;
use std::{
    fs,
    sync::{Arc, Barrier, Mutex},
};
struct Fixture {
    dir: PathBuf,
    root: PathBuf,
    h: host::Host,
    request: Value,
    at: u64,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("taskq-slots-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let root = dir.join("project");
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("taskq.json"), "{}").unwrap();
        fs::write(dir.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(
            dir.join("host.json"),
            json!({"limits":{"codex":1},"admission":{"max":1},"load_gate":{"enabled":false}})
                .to_string(),
        )
        .unwrap();
        fs::create_dir(root.join(".taskq")).unwrap();
        fs::write(root.join(".taskq/focus"), "off").unwrap();
        let h = host::Host::new(Ok(host::Policy::read(&dir).unwrap()));
        let at = now().unwrap();
        let native = coreq::process::identity(std::process::id())
            .unwrap()
            .unwrap();
        fs::write(dir.join("service"),json!({"schema":1,"pid":native.pid,"started":native.started,"instance":h.instance,"at":at,"version":crate::platform::VERSION,"slots_protocol":1}).to_string()).unwrap();
        let request = json!({"binding":{"schema":SCHEMA,"caller":"pilotq","op_id":"turn-1","root":root,"runtime":"codex","pid":native.pid,"started":native.started,"boot":10,"expires_at":at+300},"key":"a".repeat(64)});
        Self {
            dir,
            root,
            h,
            request,
            at,
        }
    }
    fn call(&self, action: &str) -> Value {
        exchange(&self.dir, action, &self.request).unwrap()
    }
    fn tick(&mut self, live: Res<bool>) {
        reconcile(&self.dir, &mut self.h, self.at, Ok(10), |_| live.clone()).unwrap();
    }
    fn stored(&self) -> Value {
        resources::try_with(&self.dir, |s| {
            Ok(s.ops
                .values()
                .find(|o| o.bound.get("local_slot").is_some())
                .unwrap()
                .receipt
                .clone())
        })
        .unwrap()
    }
}
// Chain: R1 R3 R9 R12 §3 — PilotQ and a worker contend for the same last slot; sync cannot erase a grant.
// Risk: a second capacity pool or unsynchronized start oversubscribes, changes policy or changes focus.
#[test]
fn pilotq_worker_share_one_authority() {
    for pilotq_first in [false, true] {
        let f = Fixture::new();
        let policy = fs::read(f.dir.join("host.json")).unwrap();
        f.call("reserve");
        let shared = Arc::new(Mutex::new(host::Host::new(Ok(
            host::Policy::read(&f.dir).unwrap()
        ))));
        let barrier = Arc::new(Barrier::new(3));
        let mut joins = vec![];
        for external in [pilotq_first, !pilotq_first] {
            let (shared, barrier, root, dir, at) = (
                shared.clone(),
                barrier.clone(),
                f.root.clone(),
                f.dir.clone(),
                f.at,
            );
            joins.push(std::thread::spawn(move || {
                barrier.wait();
                let mut h = shared.lock().unwrap();
                if external {
                    reconcile(&dir, &mut h, at, Ok(10), |_| Ok(true)).unwrap();
                } else {
                    let cap = h.policy.as_ref().unwrap().cap.clone();
                    let _ = h.reserve(&root, 1, 77, "codex", &cap, false, 30);
                }
            }));
        }
        barrier.wait();
        for j in joins {
            j.join().unwrap();
        }
        let mut h = shared.lock().unwrap();
        assert_eq!(h.occupied(), 1);
        assert_eq!(h.used()["codex"], 1);
        let r = f.stored();
        let external = r["state"] == "reserved";
        h.sync(
            &f.root,
            if external {
                vec![]
            } else {
                vec![(1, 77, "codex".into())]
            },
        );
        assert_eq!(h.occupied(), 1);
        let cap = h.policy.as_ref().unwrap().cap.clone();
        assert!(h.reserve(&f.root, 2, 78, "codex", &cap, false, 60).is_err());
        assert_eq!(fs::read(f.dir.join("host.json")).unwrap(), policy);
        assert_eq!(
            fs::read_to_string(f.root.join(".taskq/focus")).unwrap(),
            "off"
        );
    }
}
// Chain: R9 R12 §3 A3 — a queued manager gets a round-robin turn despite a worker backlog.
// Risk: project readiness updates erase external demand and starve PilotQ until its deadline.
#[test]
fn queued_slot_is_admitted_with_a_ready_worker_project() {
    let mut f = Fixture::new();
    let worker = f.root.join("worker");
    f.h.ready(&worker, true);
    f.call("reserve");
    f.h.ready(&f.root, false);
    f.tick(Ok(true));
    let out = f.stored();
    assert_eq!(
        out["state"], "reserved",
        "queued external demand must join round-robin: {out}"
    );
    assert_eq!(f.h.occupied(), 1);
    let cap = f.h.policy.as_ref().unwrap().cap.clone();
    assert!(
        f.h.reserve(&worker, 1, 77, "codex", &cap, true, 30)
            .unwrap_err()
            .contains("capacity full")
    );
}
// Chain: R9 R12 §3 — a valid host.json cannot clear the host loop's legacy/startup barrier.
// Risk: reconciling PilotQ admission also enables workers later in the same host pass.
#[test]
fn external_admission_preserves_guarded_policy() {
    let mut f = Fixture::new();
    let cap = f.h.policy.as_ref().unwrap().cap.clone();
    let barrier = "legacy/startup barrier".to_owned();
    f.h.reload(Err(barrier.clone()));
    f.call("reserve");
    for _ in 0..2 {
        f.tick(Ok(true));
        assert_eq!(f.h.policy, Err(barrier.clone()));
        let out = f.stored();
        assert_eq!(out["state"], "unknown");
        assert_eq!(out["occupied"], false);
        assert!(out["reason"].as_str().unwrap().contains(&barrier));
        assert_eq!(
            f.h.reserve(&f.root, 1, 77, "codex", &cap, false, 30),
            Err(barrier.clone())
        );
        assert_eq!(f.h.occupied(), 0);
    }
}
// Chain: R9 R12 §3 — existing policy gates external callers without changing policy or fabricating healthy reads.
#[test]
fn external_admission_keeps_each_existing_barrier() {
    for case in [
        "runtime",
        "project",
        "aggregate",
        "load",
        "hold",
        "recovery",
        "config",
        "project-config",
    ] {
        let mut f = Fixture::new();
        let mut policy =
            json!({"limits":{"codex":2},"admission":{"max":2},"load_gate":{"enabled":false}});
        match case {
            "runtime" => policy["limits"]["codex"] = json!(0),
            "project" => fs::write(f.root.join("taskq.json"), r#"{"limits":{"codex":0}}"#).unwrap(),
            "aggregate" => policy["admission"]["max"] = json!(1),
            "load" => policy["load_gate"]["enabled"] = json!(true),
            "project-config" => fs::write(f.root.join("taskq.json"), "[]").unwrap(),
            "hold" => f.h.held = Some("operator emergency hold".into()),
            "recovery" => f.h.rebuilding = true,
            _ => {}
        }
        fs::write(
            f.dir.join("host.json"),
            if case == "config" {
                "{".into()
            } else {
                policy.to_string()
            },
        )
        .unwrap();
        f.h.reload(host::Policy::read(&f.dir));
        f.call("reserve");
        if case == "aggregate" {
            // A worker occupies a different runtime; the host aggregate still includes it.
            f.h.sync(&f.root, vec![(1, 77, "claude".into())]);
        }
        f.tick(Ok(true));
        let out = f.stored();
        assert_eq!(out["occupied"], false, "{case}: {out}");
        assert_eq!(
            out["state"],
            if ["load", "recovery", "config", "project-config"].contains(&case) {
                "unknown"
            } else {
                "busy"
            },
            "{case}: {out}"
        );
        let reason = out["reason"].as_str().unwrap();
        assert!(
            reason.contains(match case {
                "runtime" => "host capacity",
                "project" => "project capacity",
                "aggregate" => "aggregate capacity",
                "load" => "sample",
                "hold" => "emergency hold",
                "recovery" => "recovery",
                "project-config" => "taskq.json",
                _ => "host.json",
            }),
            "{case}: {out}"
        );
    }
}
// Chain: §3 R12 — duplicate/lost responses, foreign release, cancellation and restart reconcile one identity.
// Risk: retries launch twice, cancellation races admission, or a vanished manager frees surviving tools.
#[test]
fn retries_release_and_recovery_keep_ownership() {
    let mut f = Fixture::new();
    f.call("reserve");
    f.call("reserve");
    f.tick(Ok(true));
    assert_eq!(f.call("observe")["state"], "reserved");
    assert_eq!(f.h.occupied(), 1);
    assert_eq!(f.call("reserve")["state"], "reserved");
    for field in ["key", "caller", "runtime", "op_id", "started"] {
        let mut other = f.request.clone();
        if field == "key" {
            other[field] = json!("b".repeat(64));
        } else {
            other["binding"][field] = json!("foreign");
        }
        assert!(
            exchange(&f.dir, "release", &other).is_err(),
            "foreign {field}"
        );
        assert_eq!(f.h.occupied(), 1);
    }
    let mut recovered = host::Host::new(Ok(host::Policy::read(&f.dir).unwrap()));
    reconcile(
        &f.dir,
        &mut recovered,
        f.at,
        Err("boot unavailable".into()),
        |_| Err("native read unavailable".into()),
    )
    .unwrap();
    assert_eq!(recovered.occupied(), 1);
    assert_eq!(f.stored()["state"], "unknown");
    f.h = recovered;
    f.tick(Ok(false));
    assert_eq!(f.h.occupied(), 1);
    assert_eq!(f.stored()["state"], "unknown");
    assert_eq!(
        f.call("release")["state"],
        "unknown",
        "old host generation cannot acknowledge a release"
    );
    assert_eq!(f.stored()["state"], "releasing");
    let mut record = crate::data::json(&f.dir.join("service")).unwrap().unwrap();
    record["instance"] = json!(f.h.instance);
    fs::write(f.dir.join("service"), record.to_string()).unwrap();
    assert_eq!(f.call("release")["state"], "releasing");
    f.tick(Ok(false));
    assert_eq!(f.h.occupied(), 0);
    assert_eq!(f.call("release")["state"], "released");
    assert_eq!(f.call("reserve")["state"], "released");
    let retained_until =
        resources::try_with(&f.dir, |s| Ok(s.ops.values().next().unwrap().expires_at)).unwrap();
    f.h = host::Host::new(Ok(host::Policy::read(&f.dir).unwrap()));
    f.tick(Ok(false));
    assert_eq!(
        f.stored()["instance"],
        f.h.instance,
        "a restarted host must reacknowledge a terminal slot"
    );
    assert_eq!(
        resources::try_with(&f.dir, |s| Ok(s.ops.values().next().unwrap().expires_at)).unwrap(),
        retained_until,
        "readback cannot extend terminal retention"
    );
    f.at += RETAIN + 1;
    f.tick(Ok(false));
    assert!(resources::try_with(&f.dir, |s| Ok(s.ops.is_empty())).unwrap());
}
// Chain: §3 R12 — never-started calls expire/cancel; admitted calls survive expiry until release or proven reboot.
#[test]
fn abandoned_calls_and_store_failures_stay_fenced() {
    let mut f = Fixture::new();
    f.call("reserve");
    f.call("release");
    f.tick(Ok(true));
    assert_eq!(f.stored()["state"], "released");
    assert_eq!(f.h.occupied(), 0);
    let mut f = Fixture::new();
    f.call("reserve");
    f.at += 301;
    f.tick(Ok(true));
    assert_eq!(f.stored()["state"], "denied");
    assert_eq!(f.h.occupied(), 0);
    let mut f = Fixture::new();
    f.call("reserve");
    f.tick(Ok(false));
    assert_eq!(f.stored()["state"], "denied");
    let mut f = Fixture::new();
    f.call("reserve");
    f.tick(Err("native unsupported".into()));
    assert_eq!(f.stored()["state"], "unknown");
    assert_eq!(f.h.occupied(), 0);
    f.tick(Ok(true));
    assert_eq!(f.h.occupied(), 1);
    f.at += 301;
    f.tick(Ok(false));
    assert_eq!(f.h.occupied(), 1);
    let path = resources::dir(&f.dir).join("state.json");
    let original = fs::read(&path).unwrap();
    fs::write(
        resources::dir(&f.dir).join("state.json.pending"),
        "interrupted",
    )
    .unwrap();
    assert!(tick(&f.dir, &mut f.h).is_err());
    assert_eq!(f.h.occupied(), 1);
    fs::remove_file(resources::dir(&f.dir).join("state.json.pending")).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(
        reconcile(&f.dir, &mut f.h, f.at, Ok(10), |_| Ok(false))
            .unwrap_err()
            .contains("disappeared")
    );
    assert_eq!(f.h.occupied(), 1);
    fs::write(&path, &original).unwrap();
    reconcile(&f.dir, &mut f.h, f.at, Ok(f.at as i64), |_| Ok(false)).unwrap();
    assert_eq!(f.h.occupied(), 0);
    assert_eq!(f.stored()["state"], "released");
}
// Chain: R9 R12 §3 — protected native launches need authenticated fresh admission, not copied receipt text.
#[test]
fn protected_launch_keeps_slot_identity_and_unknown_occupancy() {
    let mut f = Fixture::new();
    let boot = crate::platform::boot_time().unwrap();
    f.request["binding"]["boot"] = json!(boot);
    f.call("reserve");
    reconcile(&f.dir, &mut f.h, f.at, Ok(boot), |_| Ok(true)).unwrap();
    let dir = f.dir.clone();
    let check = |request: &Value| {
        resources::within(&dir, resources::WAIT, |s| {
            reserved(&dir, s, request, "codex")
        })
    };
    assert!(check(&f.request).is_ok());
    let mut changed = f.request.clone();
    changed["key"] = json!("b".repeat(64));
    assert!(check(&changed).is_err());
    changed = f.request.clone();
    changed["binding"]["started"] = json!("foreign");
    assert!(check(&changed).is_err());
    let mut service = crate::data::json(&f.dir.join("service")).unwrap().unwrap();
    let saved = service.clone();
    service["instance"] = json!("foreign");
    fs::write(f.dir.join("service"), service.to_string()).unwrap();
    assert!(check(&f.request).is_err());
    fs::write(f.dir.join("service"), saved.to_string()).unwrap();
    reconcile(&f.dir, &mut f.h, f.at, Ok(boot), |_| {
        Err("native liveness denied".into())
    })
    .unwrap();
    assert!(check(&f.request).is_err());
    assert_eq!(f.h.occupied(), 1);
    reconcile(&f.dir, &mut f.h, f.at, Ok(boot), |_| Ok(true)).unwrap();
    assert!(check(&f.request).is_ok());
    f.call("release");
    assert!(check(&f.request).is_err());
    assert_eq!(
        f.h.occupied(),
        1,
        "cancellation does not attest tool exit until host readback"
    );
}
