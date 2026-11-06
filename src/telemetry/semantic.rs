//! Product metadata types and their shared-store observations.
use serde_json::{Value, json};

type Res<T> = Result<T, String>;

#[derive(Clone, serde::Serialize)]
#[serde(untagged)]
pub(crate) enum Sample {
    Operation(crate::profile::Record),
    Stage(crate::profile::Stage),
    Board(crate::profile::BoardOp),
    Transition(crate::profile::Transition),
    Requests(crate::requests::Usage),
}

impl Sample {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Operation(_) => "operation",
            Self::Stage(_) => "stage",
            Self::Board(_) => "board",
            Self::Transition(_) => "transition",
            Self::Requests(_) => "requests",
        }
    }
    fn validate(&self) -> Res<()> {
        let valid = match self {
            Self::Operation(r) => r.valid(),
            Self::Stage(r) => r.valid(),
            Self::Board(r) => r.valid(),
            Self::Transition(r) => r.valid(),
            Self::Requests(r) => {
                r.validate()?;
                true
            }
        };
        if !valid {
            return Err("invalid profile observation".into());
        }
        Ok(())
    }
    pub(crate) fn canonical(&self) -> Res<Value> {
        self.validate()?;
        serde_json::to_value(self).map_err(|e| e.to_string())
    }
    pub(crate) fn decode(kind: &str, value: Value) -> Res<Self> {
        let sample = match kind {
            "operation" => serde_json::from_value(value).map(Self::Operation),
            "stage" => serde_json::from_value(value).map(Self::Stage),
            "board" => serde_json::from_value(value).map(Self::Board),
            "transition" => serde_json::from_value(value).map(Self::Transition),
            "requests" => serde_json::from_value(value).map(Self::Requests),
            _ => return Err("unknown semantic kind".into()),
        }
        .map_err(|e| e.to_string())?;
        sample.validate()?;
        Ok(sample)
    }
}

fn fields(kind: &str) -> Res<&'static [(&'static str, &'static str)]> {
    Ok(match kind {
        "operation" => &[
            ("op", "operation"),
            ("us", "duration_us"),
            ("error", "error"),
            ("at_ms", "started_ms"),
            ("counts", "counts"),
        ],
        "stage" => &[
            ("host", "host"),
            ("open", "open"),
            ("project", "project"),
            ("stage", "operation"),
            ("task", "task"),
            ("attempt", "attempt"),
            ("rt", "runtime"),
            ("us", "duration_us"),
            ("outcome", "outcome"),
            ("error", "error"),
            ("at_ms", "started_ms"),
            ("calls", "calls"),
            ("tokens", "tokens"),
            ("input_tokens", "input_tokens"),
            ("cache_read_tokens", "cache_read_tokens"),
            ("cache_write_tokens", "cache_write_tokens"),
            ("model", "model"),
            ("output_tokens", "output_tokens"),
            ("unknown", "usage_status"),
        ],
        "board" => &[
            ("op", "operation"),
            ("us", "duration_us"),
            ("outcome", "outcome"),
            ("error", "error"),
            ("remaining", "remaining"),
            ("at_ms", "started_ms"),
        ],
        "transition" => &[
            ("project", "project"),
            ("task", "task"),
            ("entry", "entry"),
            ("from", "from"),
            ("to", "to"),
            ("prev_s", "previous_seconds"),
            ("rt", "runtime"),
            ("host", "host"),
            ("at_ms", "started_ms"),
            ("counts", "counts"),
        ],
        "requests" => &[("at_ms", "started_ms")],
        _ => return Err("unknown semantic kind".into()),
    })
}
pub(super) fn encode(kind: &str, original: Value) -> Res<Value> {
    let mut value = json!({"product":"taskq","version":null,"kind":kind,"operation":if kind=="requests" {"requests.summary"} else {"queue.transition"},
        "trace":null,"span":null,"started_ms":null,"outcome":"unknown","error":"unknown",
        "unknown":{"version":"not-recorded","trace":"not-recorded","span":"not-recorded","started_ms":"not-recorded"}});
    for (old, new) in fields(kind)? {
        if let Some(field) = original.get(*old) {
            value[*new] = field.clone();
        }
    }
    if !value["started_ms"].is_null() {
        value["unknown"]
            .as_object_mut()
            .unwrap()
            .remove("started_ms");
    }
    for name in [
        "duration_us",
        "remaining",
        "previous_seconds",
        "calls",
        "tokens",
    ] {
        if value.get(name).is_some_and(Value::is_null) {
            value["unknown"][name] = original
                .get("unknown")
                .cloned()
                .unwrap_or(json!("not-recorded"));
        }
    }
    // gap G3: a stage without model, output tokens or a context split says why (its usage status, else the runtime reported none)
    if kind == "stage" {
        for name in [
            "model",
            "output_tokens",
            "input_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
        ] {
            if value[name].is_null() {
                value[name] = Value::Null;
                value["unknown"][name] = original
                    .get("unknown")
                    .cloned()
                    .unwrap_or(json!("not-reported"));
            }
        }
    }
    if kind == "operation" {
        value["outcome"] = json!(if value["error"] == "none" {
            "ok"
        } else {
            "failed"
        });
    }
    if let Some(trace) = original.get("trace") {
        for (old, new) in [
            ("run", "run_id"),
            ("process", "process_id"),
            ("span", "span_id"),
            ("parent", "parent_id"),
        ] {
            value[new] = trace[old].clone();
            if value[new].is_null() {
                value["unknown"][new] = json!("not-recorded");
            }
        }
        value["unknown"]["trace"] = json!("numeric-trace-only");
        value["unknown"]["span"] = json!("numeric-trace-only");
    }
    if kind == "requests" {
        value["source"] = json!(
            original["source"]
                .as_str()
                .ok_or("missing request source")?
                .replace(' ', ".")
        );
        let mut counts = serde_json::Map::new();
        for (name, count) in original["ops"]
            .as_object()
            .ok_or("missing request counts")?
        {
            let (method, path) = name.split_once(' ').ok_or("invalid request operation")?;
            counts.insert(count_key(method, path), count.clone());
        }
        value["counts"] = Value::Object(counts);
    }
    Ok(value)
}

/// The count key of request operation `<method> <path>` (§ 4, #1621): DashQ reads `:`-joined identifiers, so path
/// segments join with `.`, an id segment `*` or a word over 20 bytes reads `ID` (a path stays within 64 bytes) and the
/// API root `ROOT`. Older records spelled `*` as `:`; [`decoded`] still reads them.
/// An unbilled `<op> 304` (#1985) appends segment `304`, which no operation path holds: numbers read `N`.
fn count_key(method: &str, path: &str) -> String {
    let (path, unbilled) = path
        .strip_suffix(" 304")
        .map_or((path, ""), |p| (p, ".304"));
    if path.is_empty() {
        return format!("{method}:ROOT{unbilled}");
    }
    format!(
        "{method}:{}{unbilled}",
        path.split('/')
            .map(|s| if s == "*" || s.len() > 20 { "ID" } else { s })
            .collect::<Vec<_>>()
            .join(".")
    )
}

pub(super) fn encoded_size(original: &Value) -> Res<usize> {
    Ok(serde_json::to_vec(original)
        .map_err(|e| e.to_string())?
        .len())
}

fn decoded(value: &Value) -> Res<Sample> {
    if value.get("group_id").is_some() {
        return Err("count summary needs complete assembly".into());
    }
    if value["product"] != "taskq" {
        return Err("foreign semantic product".into());
    }
    let kind = value["kind"].as_str().ok_or("missing semantic kind")?;
    let mut original = serde_json::Map::new();
    for (old, new) in fields(kind)? {
        if let Some(field) = value.get(*new) {
            original.insert((*old).into(), field.clone());
        }
    }
    if kind == "stage" && value["attempt"].is_null() {
        original.insert("attempt".into(), json!(0));
    }
    if kind == "operation" && value.get("process_id").is_some() {
        original.insert("trace".into(),json!({"run":value["run_id"],"process":value["process_id"],"span":value["span_id"],"parent":value["parent_id"]}));
    }
    if kind == "requests" {
        original.insert(
            "source".into(),
            json!(
                value["source"]
                    .as_str()
                    .ok_or("missing request source")?
                    .replace('.', " ")
            ),
        );
        let mut ops = serde_json::Map::new();
        for (name, count) in value["counts"]
            .as_object()
            .ok_or("missing request counts")?
        {
            let (method, path) = name.split_once(':').ok_or("invalid request count code")?;
            let (path, unbilled) = path
                .strip_suffix(".304")
                .map_or((path, ""), |p| (p, " 304"));
            let path = if path == "ROOT" {
                String::new()
            } else {
                path.split('.')
                    .map(|s| if matches!(s, "ID" | ":") { "*" } else { s })
                    .collect::<Vec<_>>()
                    .join("/")
            };
            ops.insert(format!("{method} {path}{unbilled}"), count.clone());
        }
        original.insert("ops".into(), Value::Object(ops));
    }
    Sample::decode(kind, Value::Object(original))
}

/// Report input from bounded indexed pages, never a scan of generic method payloads. Lane 0 holds rows without a
/// recorded version (imported history), lane 1 live rows; partial summary groups stay unknown.
pub(super) fn visit_samples(
    store: Option<&coreq::telemetry::Reader>,
    kinds: &[&str],
    mut accept: impl FnMut(usize, Sample),
) -> Res<()> {
    let Some(store) = store else { return Ok(()) };
    let mut reader = coreq::telemetry::Assembler::default();
    let mut after = 0;
    loop {
        let page = store.export_kinds(kinds, 0, after, 1024)?;
        for row in page["events"].as_array().ok_or("invalid semantic page")? {
            if let Some(value) = reader.push(row["observation"].clone())? {
                accept(usize::from(!value["version"].is_null()), decoded(&value)?);
            }
        }
        after = page["next"].as_i64().ok_or("missing semantic cursor")?;
        if page["more"] == false {
            break;
        }
    }
    reader.finish()
}

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod tests;
