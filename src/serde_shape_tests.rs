//! The serde model on the wire: field names and JSON types of every type other readers or older files meet, and the exact
//! strings of its enums. The golden file changes only with a deliberate format change.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// Every string and number gives way to its JSON type; an array keeps the shape of its first item.
fn shape(v: &Value) -> Value {
    match v {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("boolean"),
        Value::Number(n) => json!(if n.is_f64() { "number" } else { "integer" }),
        Value::String(_) => json!("string"),
        Value::Array(a) => Value::Array(a.first().map(shape).into_iter().collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), shape(x))).collect()),
    }
}

fn of<T: serde::Serialize>(value: T) -> Value { shape(&serde_json::to_value(value).unwrap()) }

fn strings<T: serde::Serialize>(values: &[T]) -> Value { serde_json::to_value(values).unwrap() }

fn model() -> Value {
    use crate::core::{Gate, State, Task};
    let some = || Some("x".to_string());
    let task = Task {
        state: State::Doing, attempt: Some(1), role: some(), session: some(), retries: 1, card: some(), result: some(), inbox: vec![1],
        publication: some(), enqueued: some(), gate: Some(Gate::Required), pick: Some((1, "x".into())), blocker: some(), until: some(),
        stages: vec!["x".into()],
    };
    let resources = crate::resources::State {
        grants: BTreeMap::from([("x".to_string(), crate::resources::Grant {
            actor_id: "x".into(), installation_id: "x".into(), generation: 1, project_ids: vec!["x".into()], runtime_ids: vec!["x".into()],
            host_policy: true, project_limits: true, expires_at: 1, revoked: false, by: "x".into(), at: 1,
        })]),
        projects: BTreeMap::from([("x".to_string(), crate::resources::Project { root: "x".into(), revision: "x".into(), policy: json!({}), since: 1 })]),
        ops: BTreeMap::from([("x".to_string(), crate::resources::Op { bound: json!({}), receipt: json!({}), expires_at: 1, saved_at: Some(1), writing: true, sent: true })]),
    };
    let policy = crate::policy::Policy {
        revision: 1,
        grants: vec![crate::policy::Grant { runtime: "x".into(), mode: "x".into(), project: some(), by: "x".into(), at: 1 }],
    };
    let admission = crate::admission::Host {
        launched: 1,
        services: BTreeMap::from([("x".to_string(), crate::admission::Entry { version: "x".into(), at: 1, held: vec![(1, 1, "x".into())], requested: vec![(1, 1, "x".into())] })]),
    };
    let queues = crate::host::Queues {
        restart: Some(1),
        projects: vec![crate::host::Queue { project: some(), ready: 1, spawn: Some(1), waiting: Some(1), reason: some(), tasks: BTreeMap::from([(1, "x".to_string())]) }],
    };
    let execution = crate::snapshot::ExecutionStatus { progress: Some(json!({})), task: 1, attempt: 1, id: 1, through: 1, state: "x".into(), reason: "x".into() };
    let event = crate::telemetry::WorkerEvent {
        at_ms: 1, project: "x".into(), host: "x".into(), runtime: "x".into(), role: "x".into(), event: "x".into(), task: 1, attempt: 1, pid: 1, outcome: "x".into(), reason: "x".into(),
    };
    let window = crate::runtime::usage::Window { name: "x".into(), used: 1, reset: Some(1) };
    let fleet = crate::runtime::usage::Fleet { at: 1, policy: Default::default(), rows: vec!["x".into()], error: some() };
    let routing = crate::labels::Routing { labels: vec!["x".into()], prefer: "x".into() };
    json!({
        "core.Gate": strings(&[Gate::Required, Gate::Approved, Gate::Stale]),
        "core.State": strings(&[State::Ready, State::Waiting, State::Doing, State::Review, State::Ask, State::Later, State::Failed, State::Closed, State::Held]),
        "core.Task": of(task),
        "config.Delivery": strings(&[crate::config::Delivery::AutoMerge, crate::config::Delivery::ReviewPr, crate::config::Delivery::Direct]),
        "runtime.RuntimeId": strings(&[crate::runtime::RuntimeId::Claude, crate::runtime::RuntimeId::Codex, crate::runtime::RuntimeId::CodexApp, crate::runtime::RuntimeId::Antigravity]),
        "resources.State": of(resources),
        "policy.Policy": of(policy),
        "admission.Host": of(admission),
        "host.Queues": of(queues),
        "snapshot.ExecutionStatus": of(execution),
        "telemetry.WorkerEvent": of(event),
        "runtime.usage.Window": of(window),
        "runtime.usage.Fleet": of(fleet),
        "labels.Routing": of(routing),
    })
}

// Chain: A5 serde model -> stored files and heartbeats. Risk: a rename or a new skip changes a format another reader or an older file depends on.
#[test]
fn serde_model_matches_the_golden_shape() {
    let got = model();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/serde-shape.json");
    if std::env::var_os("TASKQ_BLESS").is_some() { std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap(); }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
}

#[test]
fn shape_keeps_keys_and_types_and_drops_values() {
    assert_eq!(shape(&json!({"a": 1, "b": [{"c": "x"}, {"d": 2}], "e": null, "f": 1.5, "g": true, "h": []})),
        json!({"a": "integer", "b": [{"c": "string"}], "e": "null", "f": "number", "g": "boolean", "h": []}));
}
