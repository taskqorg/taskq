//! R8/R9: one protected native app-server launch on an existing external slot, never a grant authority.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]
use crate::runtime::Cap;
use crate::{Res, policy, resources, runtime};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "taskq.app-server/1";
/// R12: transport refusals carry the numeric OS error, so a native failure is attributable, never inferred.
fn errno(e: &std::io::Error) -> String {
    e.raw_os_error()
        .map_or("errno unknown".into(), |n| format!("errno {n}"))
}
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    slot: Value,
    policy_revision: u64,
    account: String,
    #[serde(default)]
    preparation: String,
}
impl Request {
    fn parse(text: &str) -> Res<Self> {
        if text.len() > 16384 {
            return Err("app-server request exceeds 16384 bytes".into());
        }
        let r: Self =
            serde_json::from_str(text).map_err(|_| "invalid protected app-server request")?;
        if r.schema != SCHEMA
            || r.policy_revision == 0
            || !runtime::accounts::valid_label(&r.account)
            || !r.slot["binding"]["runtime"]
                .as_str()
                .is_some_and(|rt| runtime::of(rt).has(Cap::PassiveWriter))
        {
            return Err("invalid protected app-server identity".into());
        }
        Ok(r)
    }
    fn root(&self) -> Res<PathBuf> {
        let root = PathBuf::from(
            self.slot["binding"]["root"]
                .as_str()
                .ok_or("canonical slot project required")?,
        );
        if !root.is_absolute() || canonical(&root)? != root {
            return Err("canonical slot project required".into());
        }
        Ok(root)
    }
}

#[cfg(test)]
#[path = "protected_app_server_tests.rs"]
mod tests;
fn canonical(p: &Path) -> Res<PathBuf> {
    p.canonicalize()
        .map_err(|_| "protected launch path identity unknown".into())
}
fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn slot_id(r: &Request) -> Res<String> {
    let b = &r.slot["binding"];
    Ok(format!(
        "slot-{}",
        digest(
            format!(
                "{}\0{}",
                b["caller"].as_str().ok_or("slot caller unknown")?,
                b["op_id"].as_str().ok_or("slot operation unknown")?
            )
            .as_bytes()
        )
    ))
}
fn scope_directory(install: &Path, r: &Request) -> Res<PathBuf> {
    // A short socket name under the existing protected install; collisions refuse creation, never replace a scope.
    Ok(install.join("native-scopes").join(&slot_id(r)?[5..37]))
}
fn preparation(r: &Request, authority: &Value, instance: &Value) -> Value {
    json!({"schema":"taskq.app-server-preparation/1","binding":r.slot["binding"],"authority":authority,"host_instance":instance})
}
fn prepared(s: &resources::State, key: &str, r: &Request, authority: &Value) -> Res<()> {
    let op = s.ops.get(key).ok_or("slot absent; preparation unknown")?;
    let expected = preparation(r, authority, &op.receipt["instance"]);
    if r.preparation.is_empty()
        || op.bound["local_slot"]["prepared"] != expected
        || r.preparation != digest(expected.to_string().as_bytes())
    {
        return Err(
            "independent preparation missing, stale or changed; no native dispatch/input".into(),
        );
    }
    Ok(())
}

/// R12: a pre-native abort completes only on kernel proof: no native identity was ever recorded and the
/// scope's started-task count still equals the durable pre-launch baseline, so no descendant ever joined it.
fn no_spawn(launch: &Value, counts: &Value) -> bool {
    launch["native"].is_null()
        && launch["input_closed"] == true
        && counts.as_array().is_some_and(|c| c.len() == 2)
        && launch["scope"]["prelaunch_counts"] == *counts
}
fn certified(launch: &Value) -> Res<()> {
    let scope = &launch["scope"];
    let cert = &scope["certificate"];
    if launch["input_closed"] != true
        || scope["dispatch_ack"] != true
        || cert["schema"] != "taskq.native-completion/1"
        || cert["scope"] != scope["binding"]
        || cert["native"] != launch["native"]
        || cert["supervisor"] != launch["supervisor"]
        || cert["counts"][0]
            .as_u64()
            .zip(cert["counts"][1].as_u64())
            .is_none_or(|(a, b)| a.checked_sub(b) != Some(1))
        || native(&cert["supervisor"]).is_err()
        || (native(&cert["native"]).is_err() && !no_spawn(launch, &cert["counts"]))
        || scope["binding"]["coalition"]
            .as_u64()
            .is_none_or(|v| v == 0)
        || scope["binding"]["boot"].as_i64().is_none_or(|v| v <= 0)
    {
        return Err("protected descendant completion unproved; retain original slot".into());
    }
    Ok(())
}
/// R9/R12: an authenticated consumer still cannot release unproved protected descendants.
pub(crate) fn release_ready(op: &resources::Op) -> Res<()> {
    let launch = &op.bound["local_slot"]["launch"];
    if launch.is_null() {
        return Ok(());
    }
    certified(launch)?;
    if op.bound["local_slot"]["binding"]["boot"]
        .as_i64()
        .zip(launch["scope"]["binding"]["boot"].as_i64())
        .is_none_or(|(a, b)| a <= 0 || a.abs_diff(b) > 2)
    {
        return Err("protected slot/scope boot binding unknown; retain original slot".into());
    }
    if launch["scope"]["cleanup_intent"] != true
        || launch["scope"]["bootout_ack"] != true
        || launch["scope"]["cleanup_verified"] != true
    {
        return Err("protected scope cleanup unproved; retain original slot".into());
    }
    Ok(())
}
fn certify(
    s: &mut resources::State,
    key: &str,
    r: &Request,
    supervisor: &coreq::process::Identity,
    binding: &Value,
    counts: [u64; 2],
) -> Res<bool> {
    launch_record(s, key, r)?;
    let launch = &mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
    let mine = json!({"pid":supervisor.pid,"started":supervisor.started});
    if launch["supervisor"] != mine
        || launch["scope"]["binding"] != *binding
        || launch["input_closed"] != true
        || launch["scope"]["dispatch_ack"] != true
        || counts[0].checked_sub(counts[1]).is_none()
    {
        return Err("scope identity/input seal unknown; completion refused".into());
    }
    let old = &launch["scope"]["last_counts"];
    if !old.is_null()
        && (old[0].as_u64().is_none_or(|n| n > counts[0])
            || old[1].as_u64().is_none_or(|n| n > counts[1]))
    {
        return Err("scope counters regressed; completion unknown".into());
    }
    if !launch["scope"]["certificate"].is_null() {
        if launch["scope"]["certificate"]["counts"] != json!(counts) {
            return Err("sealed scope changed after certification; retain original receipt".into());
        }
        return Ok(true);
    }
    if counts[0] - counts[1] == 1 && !no_spawn(launch, &json!(counts)) {
        native(&launch["native"])?;
    }
    launch["scope"]["last_counts"] = json!(counts);
    if counts[0] - counts[1] != 1 {
        return Ok(false);
    }
    launch["scope"]["certificate"] = json!({"schema":"taskq.native-completion/1","scope":binding,"supervisor":mine,"native":launch["native"],"counts":counts});
    Ok(true)
}
fn file_digest(path: &Path) -> Res<String> {
    let mut file = fs::File::open(path).map_err(|_| "protected executable unreadable")?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut bytes = [0; 65536];
    loop {
        let n = file
            .read(&mut bytes)
            .map_err(|_| "protected executable read unknown")?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn stamp(path: &Path) -> Res<Value> {
    let m = fs::metadata(path).map_err(|_| "protected executable identity unknown")?;
    if !m.is_file() {
        return Err("protected executable is not a file".into());
    }
    let modified = m
        .modified()
        .map_err(|_| "protected executable clock unknown")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(json!([
            m.dev(),
            m.ino(),
            m.ctime(),
            m.ctime_nsec(),
            m.len(),
            format!("{modified:?}")
        ]));
    }
    #[cfg(not(unix))]
    {
        Ok(json!([
            m.len(),
            format!("{modified:?}"),
            format!(
                "{:?}",
                m.created()
                    .map_err(|_| "protected executable creation unknown")?
            )
        ]))
    }
}

struct Plan {
    authority: Value,
    access: policy::Effective,
    args: Vec<String>,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    queries: Vec<SandboxQuery>,
}
fn query_paths(install: &Path, p: &Plan) -> [(&'static str, PathBuf, bool); 6] {
    [
        ("workspace-probe", p.root.clone(), false),
        ("policy-store", install.join(policy::FILE), true),
        ("taskq-launcher", install.join("taskq"), true),
        ("native-executable", p.bin.clone(), true),
        (
            "account-config",
            Path::new(p.authority["account_home"].as_str().unwrap()).join("config.toml"),
            true,
        ),
        ("outside-workspace", p.home.clone(), true),
    ]
}
#[derive(serde::Serialize)]
struct SandboxQuery {
    category: &'static str,
    path: PathBuf,
    denied: bool,
    identity: Value,
}
fn query_identity(path: &Path) -> Res<Value> {
    let real = canonical(path)?;
    let m = fs::metadata(path).map_err(|_| "sandbox path identity unknown")?;
    if !m.is_file() && !m.is_dir() {
        return Err("sandbox path is not a file or directory".into());
    }
    #[cfg(unix)]
    let object = {
        use std::os::unix::fs::MetadataExt;
        json!([m.dev(), m.ino()])
    };
    // Windows is refused by resolve; this branch only supports portable fixtures.
    #[cfg(not(unix))]
    let object = json!(format!(
        "{:?}",
        m.created().map_err(|_| "sandbox path creation unknown")?
    ));
    Ok(
        json!({"canonical":real,"directory":m.is_dir(),"object":object,"file_stamp":if m.is_file(){stamp(path)?}else{Value::Null}}),
    )
}
impl SandboxQuery {
    fn path_error(&self, operation: &str) -> String {
        format!(
            "native sandbox path readback: operation={operation} category={} observed=unknown-or-changed; input refused",
            self.category
        )
    }
    fn unchanged(&self, operation: &str) -> Res<()> {
        if query_identity(&self.path).ok().as_ref() != Some(&self.identity) {
            return Err(self.path_error(operation));
        }
        Ok(())
    }
    #[cfg(any(test, target_os = "macos"))]
    fn check(
        &self,
        operation: &str,
        query: impl FnOnce(&Path) -> Res<(i32, Option<i32>)>,
    ) -> Res<()> {
        self.unchanged(operation)?;
        let (result, errno) = query(&self.path)?;
        self.unchanged(operation)?;
        sandbox_result(operation, self.category, result, errno, self.denied)
    }
}
// Resolve only host inputs. The request's revision/account are equality requirements, never grants.
fn resolve(install: &Path, home: &Path, r: &Request, os: &str, bin: &Path) -> Res<Plan> {
    if os != "macos" {
        return Err(
            "protected app-server supports qualified macOS Codex only; no Full fallback".into(),
        );
    }
    let root = r.root()?;
    let install = canonical(install)?;
    let home = canonical(home)?;
    let bin = canonical(bin)?;
    if !crate::host::projects(&install)?.contains(&root) {
        return Err("protected project is not registered".into());
    }
    let p = policy::load(&install)?;
    let grants: Vec<_> = p
        .grants
        .iter()
        .filter(|g| {
            runtime::of(&g.runtime).has(Cap::PassiveWriter) && g.project.as_deref() == root.to_str()
        })
        .collect();
    let g = grants
        .first()
        .filter(|_| grants.len() == 1)
        .ok_or("explicit project Codex Workspace grant required")?;
    if p.revision != r.policy_revision
        || g.mode != "workspace"
        || g.by.trim().is_empty()
        || g.by == "unknown"
        || g.at == 0
    {
        return Err("project Workspace authority missing, stale or mismatched".into());
    }
    let host = crate::host::Policy::read(&install)?;
    let accounts = runtime::accounts::Host::read(&install);
    if let Some(state) = crate::data::json(&install.join(runtime::accounts::STATE))? {
        for limits in state
            .as_object()
            .ok_or("host account limits unknown")?
            .values()
        {
            for limit in limits
                .as_object()
                .ok_or("host account limits unknown")?
                .values()
            {
                if limit["limited"].as_i64().is_none()
                    || (!limit["until"].is_null() && limit["until"].as_i64().is_none())
                {
                    return Err("host account limits unknown; no account fallback".into());
                }
            }
        }
    }
    // A poll may rank another account first between preparation and launch; the prepared account stays.
    if !accounts.usable(runtime::CODEX, &r.account, runtime::accounts::now()) {
        return Err("selected host account unavailable; no account fallback".into());
    }
    let base = home.join(".codex");
    let account = canonical(&if r.account == runtime::accounts::DEFAULT {
        base.clone()
    } else {
        base.join(runtime::accounts::HOMES).join(&r.account)
    })?;
    let mut access = policy::resolve(
        &Ok(p.clone()),
        &policy::Host {
            os,
            install: &install,
            root: &root,
            home: &home,
        },
        runtime::CODEX,
        &host.permission_mode,
        &host.codex,
    );
    if access.refused.is_some() || access.mode != "workspace" || access.profile.is_none() {
        return Err("protected enforcement unknown; no native input".into());
    }
    let adapter = canonical(&std::env::current_exe().map_err(|_| "adapter executable unknown")?)?;
    // Staged qualification binaries need the same write protection as an installed launcher.
    for path in [&adapter, &bin] {
        let parent = path.parent().ok_or("protected executable parent absent")?;
        if root.starts_with(parent) {
            return Err("protected executable directory overlaps the writable project".into());
        }
        access.profile.as_mut().unwrap().push_str(&format!(
            "(deny file-write* (subpath {}))\n",
            serde_json::to_string(&parent.to_string_lossy())
                .map_err(|_| "invalid executable path")?
        ));
    }
    for path in [
        home.join(".local/bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        if path.is_dir() {
            access.profile.as_mut().unwrap().push_str(&format!(
                "(deny file-write* (subpath {}))\n",
                serde_json::to_string(&canonical(&path)?.to_string_lossy())
                    .map_err(|_| "invalid host executable directory")?
            ));
        }
    }
    let control = scope_directory(&install, r)?;
    access.profile.as_mut().unwrap().push_str(&format!(
        "(deny file* (subpath {}))\n",
        serde_json::to_string(&control).map_err(|_| "invalid scope control path")?
    ));
    // App-server takes config options, not exec's sandbox/model switches. Preserve only known host requests.
    let mut args = vec![
        "-c".into(),
        "sandbox_mode=\"danger-full-access\"".into(),
        "-c".into(),
        "approval_policy=\"never\"".into(),
    ];
    let mut it = host.codex.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-s" | "--sandbox" => {
                let v = it.next().ok_or("missing host sandbox request")?;
                if v != "danger-full-access" && v != "workspace-write" {
                    return Err("host sandbox narrowing is not qualified for app-server".into());
                }
            }
            "-m" | "--model" => {
                let v = it.next().ok_or("missing host model")?;
                args.extend([
                    "-c".into(),
                    format!(
                        "model={}",
                        serde_json::to_string(v).map_err(|_| "host model invalid")?
                    ),
                ]);
            }
            "-c" | "--config" => {
                let v = it.next().ok_or("missing host config request")?;
                if !v.starts_with("model_reasoning_effort=")
                    && v != "sandbox_workspace_write.network_access=true"
                {
                    return Err(
                        "unsupported host app-server config; existing paths unchanged".into(),
                    );
                }
                args.extend([a.clone(), v.clone()]);
            }
            _ => {
                return Err(
                    "unsupported host app-server arguments; existing paths unchanged".into(),
                );
            }
        }
    }
    let authority = json!({"project":root,"runtime":runtime::CODEX,"profile_sha256":digest(access.profile.as_ref().unwrap().as_bytes()),
        "requested":access.requested,"saved":{"revision":p.revision,"grant":g},"resolved":access.json(),
        "executable":bin,"executable_identity":stamp(&bin)?,"executable_sha256":file_digest(&bin)?,
        "adapter":{"executable":adapter,"identity":stamp(&adapter)?,"sha256":file_digest(&adapter)?,"version":crate::platform::VERSION},
        "account":r.account,"account_home":account,"args":args,"scope_directory":control});
    let mut plan = Plan {
        authority,
        access,
        args,
        root,
        home,
        bin,
        queries: vec![],
    };
    for (category, path, denied) in query_paths(&install, &plan) {
        let mut q = SandboxQuery {
            category,
            path,
            denied,
            identity: Value::Null,
        };
        q.identity = query_identity(&q.path).map_err(|_| q.path_error("bind"))?;
        if q.identity["directory"]
            != json!(category == "workspace-probe" || category == "outside-workspace")
        {
            return Err(q.path_error("bind"));
        }
        plan.queries.push(q);
    }
    plan.authority["sandbox_paths"] = json!(plan.queries);
    Ok(plan)
}

fn launch_record<'a>(s: &'a resources::State, key: &str, r: &Request) -> Res<&'a Value> {
    let launch = &s
        .ops
        .get(key)
        .ok_or("slot absent; execution unknown")?
        .bound["local_slot"]["launch"];
    if launch["schema"] != SCHEMA
        || launch["policy_revision"] != r.policy_revision
        || launch["account"] != r.account
    {
        return Err("launch identity absent, corrupt or mismatched; never retry launch".into());
    }
    Ok(launch)
}
fn consume(
    s: &mut resources::State,
    key: &str,
    r: &Request,
    authority: &Value,
    supervisor: &coreq::process::Identity,
) -> Res<()> {
    prepared(s, key, r, authority)?;
    let op = s.ops.get_mut(key).ok_or("slot absent; no launch")?;
    if !op.bound["local_slot"]["launch"].is_null() {
        return Err(
            "slot launch already consumed; observe the same operation, never replay".into(),
        );
    }
    op.bound["local_slot"]["launch"] = json!({"schema":SCHEMA,"policy_revision":r.policy_revision,"account":r.account,
        "authority":authority,"supervisor":{"pid":supervisor.pid,"started":supervisor.started},"state":"unknown",
        "host_instance":op.receipt["instance"],"cancel_requested":false,"input_closed":false,
        "reason":"launch intent consumed; native execution not yet verified"});
    Ok(())
}
fn input_gate(s: &resources::State, key: &str, r: &Request) -> Res<()> {
    let launch = launch_record(s, key, r)?;
    if launch["cancel_requested"] != false
        || launch["input_closed"] != false
        || launch["host_instance"]
            .as_str()
            .filter(|v| !v.is_empty())
            .is_none()
        || launch["host_instance"] != s.ops[key].receipt["instance"]
    {
        return Err(
            "launch cancelled or host generation changed; input refused, retain slot".into(),
        );
    }
    Ok(())
}
fn bound_gate(
    s: &resources::State,
    key: &str,
    r: &Request,
    authority: &Value,
    id: &coreq::process::Identity,
    supervisor: &coreq::process::Identity,
    effective: Option<&Value>,
) -> Res<()> {
    input_gate(s, key, r)?;
    let record = launch_record(s, key, r)?;
    if record["authority"] != *authority
        || native(&record["native"])? != *id
        || native(&record["supervisor"])? != *supervisor
        || effective.is_some_and(|e| {
            record["state"] != "verified-effective" || record["verified_effective"] != *e
        })
    {
        return Err("durable launch binding changed or uncommitted; input refused".into());
    }
    Ok(())
}
fn cancel(s: &mut resources::State, key: &str, r: &Request) -> Res<Value> {
    launch_record(s, key, r)?;
    let launch = &mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
    launch["cancel_requested"] = json!(true);
    Ok(launch.clone())
}
fn native(value: &Value) -> Res<coreq::process::Identity> {
    Ok(coreq::process::Identity {
        pid: value["pid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or("native identity unknown")?,
        started: value["started"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("native identity unknown")?
            .into(),
    })
}
fn exact(
    expected: &coreq::process::Identity,
    seen: Res<Option<coreq::process::Identity>>,
) -> Res<()> {
    if seen?.as_ref() != Some(expected) {
        return Err("native incarnation absent or changed; retain slot and reconcile".into());
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn sandbox_result(
    operation: &str,
    category: &str,
    result: i32,
    errno: Option<i32>,
    denied: bool,
) -> Res<()> {
    if result < 0 || (result != 0) != denied {
        let expected = if denied { "denied" } else { "allowed" };
        let observed = if result < 0 {
            "unknown"
        } else if result == 0 {
            "allowed"
        } else {
            "denied"
        };
        let errno = errno.map_or("none".into(), |v| v.to_string());
        return Err(format!(
            "native sandbox readback: operation={operation} category={category} expected={expected} observed={observed} return={result} errno={errno}; input refused"
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn native_output(
    mut socket: std::os::unix::net::UnixStream,
) -> Res<(
    std::os::fd::OwnedFd,
    std::thread::JoinHandle<std::io::Result<bool>>,
)> {
    let (mut reader, writer) =
        std::io::pipe().map_err(|_| "native output pipe unavailable; retain scope")?;
    // Native code must never inherit the duplex socket that carries input before validation.
    // Drain to native EOF even after the frontend is lost: delivery becomes false, never an aborted certification.
    let pump = std::thread::Builder::new()
        .spawn(move || {
            use std::io::{Read, Write};
            let (mut bytes, mut delivered) = (vec![0u8; 65536], true);
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => return Ok(delivered),
                    Ok(n) => {
                        if delivered && socket.write_all(&bytes[..n]).is_err() {
                            delivered = false;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
        })
        .map_err(|_| "native output forwarding unavailable; retain scope")?;
    Ok((writer.into(), pump))
}

pub(crate) fn versions_in_use(install: &Path) -> Res<Vec<String>> {
    resources::try_with(install, |s| {
        s.ops
            .values()
            .filter(|op| {
                !op.bound["local_slot"]["launch"].is_null() && op.receipt["occupied"] != false
            })
            .map(|op| {
                op.bound["local_slot"]["launch"]["authority"]["adapter"]["version"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .map(String::from)
                    .ok_or_else(|| "external adapter release identity unknown".into())
            })
            .collect()
    })
}

pub fn command(args: &crate::cli::Args) -> Res<()> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {
        return Err("a worker cannot launch or control external app servers".into());
    }
    if args.pos.len() != 2
        || args
            .flags
            .iter()
            .any(|(k, v)| !["action", "request"].contains(&k.as_str()) || v.len() != 1)
    {
        return Err("usage: service app-server --action prepare|launch|observe|cancel|reconcile --request JSON".into());
    }
    #[cfg(target_os = "macos")]
    {
        mac::dispatch(args.need("action")?, args.need("request")?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("protected app-server supports qualified macOS Codex only; no Full fallback".into())
    }
}

#[cfg(target_os = "macos")]
#[path = "protected_scope.rs"]
mod scope;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::external_slots;
    use std::{
        ffi::{CStr, CString},
        io::{self, Write},
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
        process::Stdio,
        time::{Duration, Instant},
    };

    // Sandbox SPI declarations match WebKit's Source/WTF/wtf/spi/darwin/SandboxSPI.h.
    #[link(name = "sandbox")]
    unsafe extern "C" {
        fn sandbox_check(pid: i32, operation: *const libc::c_char, filter: i32, ...) -> i32;
        static SANDBOX_CHECK_NO_REPORT: i32;
    }

    pub(super) fn host_paths() -> Res<(PathBuf, PathBuf)> {
        let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buf = vec![0u8; 65536];
        let status = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if status != 0 || result.is_null() {
            return Err("host account identity unknown".into());
        }
        let home = canonical(Path::new(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr((*result).pw_dir) }.to_bytes(),
        )))?;
        let install = canonical(&home.join(".local/share/taskq"))?;
        for (name, expected) in [
            ("HOME", &home),
            ("TASKQ_INSTALL", &install),
            (runtime::CODEX_DIR_VAR, &home.join(".codex")),
        ] {
            if let Some(value) = std::env::var_os(name) {
                if canonical(Path::new(&value))? != canonical(expected)? {
                    return Err("ambient host/account override cannot select authority".into());
                }
            }
        }
        Ok((install, home))
    }
    pub(super) fn executable(home: &Path) -> Res<PathBuf> {
        // shortcut: standard host CLI locations; use host-owned runtime resolution if custom installations need support.
        let defaults = std::ffi::OsStr::new("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        let path = runtime::user_bin_path(defaults, Some(home.as_os_str()))
            .unwrap_or_else(|| defaults.to_os_string());
        runtime::agent(runtime::RuntimeId::CodexApp.name())?.executable(Some(&path))
    }
    pub(super) fn live(id: &coreq::process::Identity) -> Res<()> {
        exact(
            id,
            coreq::process::identity(id.pid).map_err(|_| "native liveness unknown".into()),
        )
    }
    fn child_of(id: &coreq::process::Identity, supervisor: &coreq::process::Identity) -> Res<()> {
        live(id)?;
        live(supervisor)?;
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n = unsafe {
            libc::proc_pidinfo(
                id.pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if n != size || unsafe { info.assume_init() }.pbi_ppid != supervisor.pid {
            return Err("native child ownership unknown or mismatched; slot retained".into());
        }
        live(id)?;
        live(supervisor)
    }
    pub(super) fn image(id: &coreq::process::Identity, path: &Path) -> Res<()> {
        live(id)?;
        let mut buf = [0u8; 4096];
        let n =
            unsafe { libc::proc_pidpath(id.pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n <= 0 {
            return Err("native executable readback unknown".into());
        }
        let end = buf
            .iter()
            .position(|b| *b == 0)
            .ok_or("native executable path incomplete")?;
        if canonical(Path::new(std::ffi::OsStr::from_bytes(&buf[..end])))? != path {
            return Err("native executable has not matched the selected image".into());
        }
        live(id)
    }
    fn check(pid: u32, operation: &str, q: &SandboxQuery) -> Res<()> {
        q.check(operation, |path| {
            let op = CString::new(operation).map_err(|_| "invalid sandbox operation")?;
            let path =
                CString::new(path.as_os_str().as_bytes()).map_err(|_| "invalid sandbox path")?;
            let result = unsafe {
                *libc::__error() = 0;
                sandbox_check(
                    pid as i32,
                    op.as_ptr(),
                    1 | SANDBOX_CHECK_NO_REPORT,
                    path.as_ptr(),
                )
            };
            let errno = if result < 0 {
                io::Error::last_os_error().raw_os_error()
            } else {
                None
            };
            Ok((result, errno))
        })
    }
    fn verified(id: &coreq::process::Identity, p: &Plan, scope: &Value) -> Res<Value> {
        image(id, &p.bin)?;
        for q in &p.queries {
            for &op in if q.denied {
                &["file-write-data", "file-write-create", "file-write-unlink"][..]
            } else {
                &["file-write-data"][..]
            } {
                check(id.pid, op, q)?;
            }
        }
        let control = SandboxQuery {
            category: "scope-control",
            path: PathBuf::from(
                p.authority["scope_directory"]
                    .as_str()
                    .ok_or("scope control unknown")?,
            ),
            denied: true,
            identity: scope["directory_identity"].clone(),
        };
        for op in [
            "file-read-data",
            "file-write-data",
            "file-write-create",
            "file-write-unlink",
        ] {
            check(id.pid, op, &control)?;
        }
        scope::membership(id, &scope["binding"])?;
        image(id, &p.bin)?;
        let protected: Vec<_> = p
            .queries
            .iter()
            .filter(|q| q.denied)
            .map(|q| &q.path)
            .collect();
        Ok(
            json!({"state":"verified-effective","scope":"live native image and named effective sandbox queries; native workload qualification separate",
            "workspace_write":true,"protected_write_create_unlink_denied":protected,"reads":"existing profile","network":"existing profile"}),
        )
    }
    pub(super) fn refresh(install: &Path, home: &Path, r: &Request, original: &Value) -> Res<Plan> {
        let bin = executable(home)?;
        let p = resolve(install, home, r, "macos", &bin)?;
        if p.authority != *original {
            return Err(
                "bound project/profile/account/executable authority changed; native input refused"
                    .into(),
            );
        }
        Ok(p)
    }
    fn key(install: &Path, r: &Request) -> Res<String> {
        resources::within(install, resources::WAIT, |s| {
            external_slots::reserved(install, s, &r.slot, runtime::CODEX)
        })
    }
    pub(super) fn dispatch(action: &str, text: &str) -> Res<()> {
        if action == "scope-run" {
            return scope::run(text);
        }
        if action == "prepare" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Prepare {
                schema: String,
                slot: Value,
            }
            if text.len() > 16384 {
                return Err("preparation request exceeds 16384 bytes".into());
            }
            let input: Prepare =
                serde_json::from_str(text).map_err(|_| "invalid preparation request")?;
            if input.schema != SCHEMA {
                return Err("unsupported preparation schema".into());
            }
            let (install, home) = host_paths()?;
            let result = resources::within(&install, resources::WAIT, |s| {
                let account = runtime::accounts::Host::read(&install)
                    .active(runtime::CODEX, runtime::accounts::now())
                    .ok_or("active account unknown")?;
                let r = Request {
                    schema: input.schema.clone(),
                    slot: input.slot.clone(),
                    policy_revision: policy::load(&install)?.revision,
                    account,
                    preparation: String::new(),
                };
                let key = external_slots::reserved(&install, s, &r.slot, runtime::CODEX)?;
                let p = resolve(&install, &home, &r, "macos", &executable(&home)?)?;
                let op = s.ops.get_mut(&key).ok_or("slot absent")?;
                if !op.bound["local_slot"]["launch"].is_null() {
                    return Err("launch already consumed; never derive expected authority from a launch receipt".into());
                }
                let value = preparation(&r, &p.authority, &op.receipt["instance"]);
                let old = &op.bound["local_slot"]["prepared"];
                if !old.is_null() && old != &value {
                    return Err(
                        "original preparation changed; retain operation, no native launch".into(),
                    );
                }
                op.bound["local_slot"]["prepared"] = value.clone();
                Ok(
                    json!({"schema":SCHEMA,"state":"prepared","preparation":digest(value.to_string().as_bytes()),"policy_revision":r.policy_revision,"account":r.account,"prepared":value}),
                )
            })?;
            println!("{result}");
            return Ok(());
        }
        command(action, &Request::parse(text)?)
    }
    pub(super) fn command(action: &str, r: &Request) -> Res<()> {
        if !["launch", "observe", "cancel", "reconcile"].contains(&action) {
            return Err(
                "app-server action must be prepare, launch, observe, cancel or reconcile".into(),
            );
        }
        let (install, home) = host_paths()?;
        // Existing slot authentication, including exact payload/key, occurs before any native action.
        let observed = external_slots::exchange(&install, "observe", &r.slot)?;
        let slot_key = slot_id(r)?;
        let _ = observed;
        if action == "launch" {
            return scope::launch(&install, &home, r, &slot_key);
        }
        if action == "reconcile" {
            let result = scope::reconcile(&install, r, &slot_key)?;
            println!("{result}");
            return Ok(());
        }
        let (record, completed) = resources::within(&install, resources::WAIT, |s| {
            let record = if action == "cancel" {
                cancel(s, &slot_key, r)?
            } else {
                launch_record(s, &slot_key, r)?.clone()
            };
            Ok((
                record,
                release_ready(s.ops.get(&slot_key).ok_or("slot absent")?).is_ok(),
            ))
        })?;
        let mut receipt = json!({"schema":SCHEMA,"operation":r.slot["binding"]["op_id"],"caller":r.slot["binding"]["caller"],"launch":record,"state":"unknown"});
        if completed {
            receipt["state"] = json!("completed");
            println!("{receipt}");
            return Ok(());
        }
        let id = match native(&record["native"]) {
            Ok(id) => id,
            Err(reason) => {
                receipt["reason"] = json!(if action == "cancel" {
                    "cancellation saved before native identity; input remains closed".into()
                } else {
                    reason
                });
                println!("{receipt}");
                return Ok(());
            }
        };
        if action == "cancel" {
            // Revocation closes input, but cannot remove the original owner's exact-process stop authority.
            child_of(&id, &native(&record["supervisor"])?)?;
            image(
                &id,
                Path::new(
                    record["authority"]["executable"]
                        .as_str()
                        .ok_or("bound executable unknown")?,
                ),
            )?;
            coreq::process::terminate(&id)
                .map_err(|_| "native cancellation unconfirmed; retain slot and reconcile")?;
            receipt["reason"] = json!(
                "exact native termination requested; turn/descendant completion unproved; slot retained"
            );
        } else {
            let result = (|| -> Res<()> {
                key(&install, r)?;
                let p = refresh(&install, &home, r, &record["authority"])?;
                let supervisor = native(&record["supervisor"])?;
                live(&supervisor)?;
                child_of(&id, &supervisor)?;
                let effective = verified(&id, &p, &record["scope"])?;
                resources::within(&install, resources::WAIT, |s| {
                    bound_gate(
                        s,
                        &slot_key,
                        r,
                        &p.authority,
                        &id,
                        &supervisor,
                        Some(&effective),
                    )
                })?;
                receipt["verified_effective"] = effective;
                if record["state"] != "verified-effective" {
                    return Err("launch receipt uncommitted; execution unknown".into());
                }
                Ok(())
            })();
            match result {
                Ok(()) => receipt["state"] = json!("verified-effective"),
                Err(reason) => receipt["reason"] = json!(reason),
            }
        }
        println!("{receipt}");
        Ok(())
    }
    pub(super) fn launch_native(
        install: &Path,
        home: &Path,
        r: &Request,
        key: &str,
        mut transport: std::io::BufReader<std::os::unix::net::UnixStream>,
        stdout: Stdio,
        stderr: Stdio,
    ) -> Res<()> {
        // macOS refuses SO_RCVTIMEO with EINVAL once both socket directions are shut; bind the bound before any native spawn.
        transport
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| {
                format!(
                    "scope input transport closed before native spawn ({}); no native launch",
                    errno(&e)
                )
            })?;
        let original = resources::within(install, resources::WAIT, |s| {
            Ok(launch_record(s, key, r)?.clone())
        })?;
        let p = refresh(install, home, r, &original["authority"])?;
        let supervisor = coreq::process::identity(std::process::id())
            .map_err(|_| "supervisor identity unknown")?
            .ok_or("supervisor absent")?;
        resources::within(install, resources::WAIT, |s| {
            prepared(s, key, r, &p.authority)?;
            input_gate(s, key, r)
        })?;
        let exe = std::env::current_exe().map_err(|_| "adapter executable unknown")?;
        let env = runtime::Env {
            context: "",
            root: &p.root,
            exe: &exe,
            install,
            permission_mode: "",
            codex: &p.args,
            delivery: crate::config::Delivery::AutoMerge,
            board_env: &[],
            jobs: None,
        };
        let mut cmd = runtime::executable(p.bin.clone(), &p.access, &env)?;
        // Neither authority secrets nor an ambient environment choose native account, configuration or executable.
        cmd.env_clear();
        for name in ["PATH", "LANG", "LC_ALL", "TMPDIR", "TERM"] {
            if let Some(v) = std::env::var_os(name) {
                cmd.env(name, v);
            }
        }
        cmd.env("HOME", home);
        let spec = runtime::optimized(
            runtime::RuntimeId::CodexApp.name(),
            agentq_runtime::agent::NativeSpec {
                root: p.root.clone(),
                session: "-".into(),
                resume: false,
                args: p.args.iter().map(Into::into).collect(),
                home: Some(PathBuf::from(
                    p.authority["account_home"]
                        .as_str()
                        .ok_or("account home unknown")?,
                )),
                env: vec![],
            },
        )?;
        let mut cmd =
            runtime::agent(runtime::RuntimeId::CodexApp.name())?.app_server_command(cmd, &spec)?;
        cmd.stdin(Stdio::piped()).stdout(stdout).stderr(stderr);
        resources::within(install, resources::WAIT, |s| {
            external_slots::reserved(install, s, &r.slot, runtime::CODEX)?;
            input_gate(s, key, r)
        })?;
        let mut child = cmd
            .spawn()
            .map_err(|_| "native spawn outcome unknown; intent retained, never replay")?;
        let id = coreq::process::identity(child.id())
            .map_err(|_| "spawned native identity unknown; retain intent")?
            .ok_or("spawned native absent; retain intent")?;
        resources::within(install, resources::WAIT, |s| {
            launch_record(s, key, r)?;
            scope::membership(&id, &original["scope"]["binding"])?;
            let launch = &mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
            launch["native"] = json!({"pid":id.pid,"started":id.started});
            Ok(())
        })?;
        let mut input = child
            .stdin
            .take()
            .ok_or("native stdin unavailable; intent retained")?;
        let gate = (|| -> Res<()> {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                match image(&id, &p.bin) {
                    Ok(()) => break,
                    Err(e) => {
                        if Instant::now() >= until
                            || child
                                .try_wait()
                                .map_err(|_| "native liveness unknown")?
                                .is_some()
                        {
                            return Err(e);
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            resources::within(install, resources::WAIT, |s| {
                external_slots::reserved(install, s, &r.slot, runtime::CODEX)?;
                refresh(install, home, r, &p.authority)?;
                bound_gate(s, key, r, &p.authority, &id, &supervisor, None)?;
                child_of(&id, &supervisor)?;
                let effective = verified(&id, &p, &original["scope"])?;
                let launch = &mut s.ops.get_mut(key).ok_or("slot disappeared")?.bound["local_slot"]
                    ["launch"];
                launch["verified_effective"] = effective;
                launch["state"] = json!("verified-effective");
                launch["reason"] = Value::Null;
                Ok(())
            })
        })();
        if let Err(e) = gate {
            drop(input);
            let _ = coreq::process::terminate(&id);
            return Err(e);
        }
        let flags = unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err("bounded native input unavailable; intent retained".into());
        }
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "native liveness unknown; slot retained")?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err("native app-server exited; turn outcome unproved, slot retained".into())
                };
            }
            resources::within(install, resources::WAIT, |s| {
                external_slots::reserved(install, s, &r.slot, runtime::CODEX)?;
                input_gate(s, key, r)
            })?;
            let mut bytes = vec![0; 65536];
            let count = match transport.read(&mut bytes) {
                Ok(n) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(_) => return Err("consumer input interrupted; retain native scope".into()),
            };
            if count == 0 {
                drop(input);
                return Err(
                    "consumer input ended; retain native scope until descendants finish".into(),
                );
            }
            bytes.truncate(count);
            resources::within(install, resources::WAIT, |s| {
                external_slots::reserved(install, s, &r.slot, runtime::CODEX)?;
                refresh(install, home, r, &p.authority)?;
                let effective = verified(&id, &p, &original["scope"])?;
                bound_gate(s, key, r, &p.authority, &id, &supervisor, Some(&effective))?;
                child_of(&id, &supervisor)?;
                scope::membership(&id, &original["scope"]["binding"])?;
                let until = Instant::now() + Duration::from_secs(1);
                let mut left = bytes.as_slice();
                while !left.is_empty() {
                    match input.write(left) {
                        Ok(0) => {
                            return Err(
                                "native input disconnected; delivery unknown, never replay".into(),
                            );
                        }
                        Ok(n) => left = &left[n..],
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < until =>
                        {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => {
                            return Err(
                                "native input deadline/error; delivery unknown, never replay"
                                    .into(),
                            );
                        }
                    }
                }
                Ok(())
            })?;
        }
    }
}
