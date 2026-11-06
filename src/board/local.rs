//! R1/R4/R12: credential-authenticated, read-only local service transport.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{Mutex, OnceLock},
};

thread_local! { static PREFER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub(crate) fn prefer(value: bool) {
    PREFER.set(value);
}
pub(crate) fn preferred() -> bool {
    PREFER.get()
}

pub(super) fn read_query(method: &str, url: &str, body: Option<&Value>) -> bool {
    method == "POST"
        && url.ends_with("/graphql")
        && body.is_some_and(|v| v["query"] == github::OPEN_ISSUES)
}

fn endpoint(install: &Path, board: &str) -> std::path::PathBuf {
    install.join(format!(
        "board-reader-{}.json",
        crate::requests::key(board, "")
    ))
}

fn io(error: impl std::fmt::Display) -> Error {
    Error::new(Reason::Network, format!("local board reader: {error}"))
}
fn connection(stream: &TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(HTTP_WAIT)).map_err(io)?;
    stream.set_write_timeout(Some(HTTP_WAIT)).map_err(io)
}
fn sign(api: &Api, bytes: &[u8]) -> ring::hmac::Tag {
    ring::hmac::sign(
        &ring::hmac::Key::new(
            ring::hmac::HMAC_SHA256,
            api.credentials.borrow().auth.as_bytes(),
        ),
        bytes,
    )
}
fn request(api: &Api, value: Value) -> Option<Result<Value>> {
    if !preferred() || crate::host::active() {
        return None;
    }
    let install = api.install.as_ref()?;
    Some((|| {
        let status = crate::host::service_status(install);
        let address = match status["state"].as_str() {
            Some("running") => {
                let path = endpoint(install, &api.base);
                let value = crate::data::json(&path)?.ok_or("live service has no local board reader; upgrade at the next authorized rollout")?;
                if value["schema"] != 1 || value["board"] != api.base || value["pid"] != status["pid"] || value["started"] != status["started"] {
                    return Err("local board reader owner differs from the running service".into());
                }
                value["address"].as_str().ok_or("local board reader address missing")?.parse::<SocketAddr>().map_err(io)?
            }
            Some("exited") => return Ok(Value::Null),
            _ if !install.join("service").try_exists().map_err(io)? => return Ok(Value::Null),
            _ => return Err("local service ownership unknown; direct board fallback refused".into()),
        };
        if !address.ip().is_loopback() { return Err("local board reader is not loopback".into()); }
        api.refresh()?;
        let mut stream = TcpStream::connect_timeout(&address, HTTP_WAIT).map_err(io)?;
        connection(&stream)?;
        let payload = value.to_string();
        let wire = json!({"request":payload,"proof":sign(api, payload.as_bytes()).as_ref()});
        writeln!(stream, "{wire}").map_err(io)?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).map_err(io)?;
        let reply: Value = serde_json::from_str(&line).map_err(io)?;
        if !reply["error"].is_null() { return Err(serde_json::from_value(reply["error"].clone()).map_err(io)?); }
        if !reply["listed"].is_null() { api.proxy_listed.set(reply["listed"].as_i64()); }
        let mut budget=api.credentials.borrow().budget.borrow_mut().clone();
        budget.remaining=reply["budget"]["remaining"].as_u64();
        budget.limit=reply["budget"]["limit"].as_u64();
        budget.reset=reply["budget"]["reset"].as_i64();
        budget.resource=reply["budget"]["resource"].as_str().map(String::from);
        budget.unknown=reply["budget"]["unknown"].as_str().map(String::from);
        *api.credentials.borrow().budget.borrow_mut()=budget;
        *api.clock.borrow_mut() = reply["clock"].as_i64().ok_or("local board clock missing")?;
        api.unchanged.set(reply["unchanged"].as_bool().ok_or("local board read status missing")?);
        Ok(reply)
    })()).and_then(|result| match result {
        Ok(Value::Null) => { disclose(); None }
        other => Some(other),
    })
}
fn disclose() {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| eprintln!("Board reads: direct (local service absent)"));
}

pub(super) fn raw(
    api: &Api,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Option<Result<Reply>> {
    if method != "GET" && !read_query(method, url, body) {
        return None;
    }
    request(
        api,
        json!({"kind":"request","method":method,"url":url,"body":body}),
    )
    .map(|r| r.and_then(|r| serde_json::from_value(r["reply"].clone()).map_err(io)))
}

pub(super) fn issues(api: &Api, since: Option<i64>, tasks: bool) -> Option<Result<Vec<Issue>>> {
    request(api, json!({"kind":"list","since":since,"tasks":tasks}))
        .map(|r| r.and_then(|r| serde_json::from_value(r["issues"].clone()).map_err(io)))
}

fn answer(board: &dyn Board, api: &Api, wire: Value) -> Result<Value> {
    api.refresh()?;
    let payload = wire["request"]
        .as_str()
        .ok_or("local board request missing")?;
    let proof: Vec<u8> = serde_json::from_value(wire["proof"].clone()).map_err(io)?;
    ring::hmac::verify(
        &ring::hmac::Key::new(
            ring::hmac::HMAC_SHA256,
            api.credentials.borrow().auth.as_bytes(),
        ),
        payload.as_bytes(),
        &proof,
    )
    .map_err(|_| Error::new(Reason::Unauthorized, "local board credential refused"))?;
    let v: Value = serde_json::from_str(payload).map_err(io)?;
    let mut result = match v["kind"].as_str() {
        Some("list") => {
            let since = if v["since"].is_null() {
                None
            } else {
                Some(v["since"].as_i64().ok_or("local list since invalid")?)
            };
            let tasks = v["tasks"].as_bool().ok_or("local list filter invalid")?;
            json!({"issues":if tasks { board.tasks(since.ok_or("task-list since missing")?)? } else { board.issues(since)? },"listed":board.now()?})
        }
        Some("request") => {
            let method = v["method"].as_str().ok_or("local request method missing")?;
            let url = v["url"].as_str().ok_or("local request URL missing")?;
            let body = (!v["body"].is_null()).then_some(&v["body"]);
            if method != "GET" || body.is_some() {
                return Err(Error::new(
                    Reason::Refused,
                    "local board reader accepts only GET without a body",
                ));
            }
            json!({"reply":api.call(method, url, body.cloned())?})
        }
        _ => return Err("local board request kind invalid".into()),
    };
    result["clock"] = if result["listed"].is_null() {
        json!(*api.clock.borrow())
    } else {
        result["listed"].clone()
    };
    result["unchanged"] = json!(api.unchanged.get());
    let budget = api.credentials.borrow().budget.borrow().clone();
    result["budget"] = json!({"remaining":budget.remaining,"limit":budget.limit,"reset":budget.reset,"resource":budget.resource,"unknown":budget.unknown});
    Ok(result)
}

pub(super) fn register(
    name: &str,
    host: &str,
    repo: &str,
    tool: &coreq::Tool,
    api: &Api,
) -> Result<()> {
    if api.poll.is_none() {
        return Ok(());
    }
    static SERVING: OnceLock<Mutex<std::collections::BTreeSet<std::path::PathBuf>>> =
        OnceLock::new();
    let path = endpoint(&tool.install, &api.base);
    let mut serving = SERVING
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "local board reader registry poisoned")?;
    if serving.contains(&path) {
        return Ok(());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(io)?;
    let owner = coreq::process::identity(std::process::id())
        .map_err(io)?
        .ok_or("local board owner identity unknown")?;
    let address = listener.local_addr().map_err(io)?;
    let record = json!({"schema":1,"board":api.base,"pid":owner.pid,"started":owner.started,"address":address.to_string()});
    let (name, host, repo, tool) = (
        name.to_string(),
        host.to_string(),
        repo.to_string(),
        tool.clone(),
    );
    let root = crate::paths::find_root()?;
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let (ready, started) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("taskq-board-reader".into())
        .spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                crate::host::enter(&root, &tool.install);
                let board = super::kind(&name).and_then(|kind| {
                    Ok((
                        (kind.board)(kind.http(&host, &repo, &tool)?),
                        kind.http(&host, &repo, &tool)?,
                    ))
                });
                let _ = ready.send(board.as_ref().map(|_| ()).map_err(Clone::clone));
                let Ok((board, api)) = board else { return };
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let result = (|| {
                        connection(&stream)?;
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().map_err(io)?)
                            .read_line(&mut line)
                            .map_err(io)?;
                        answer(
                            board.as_ref(),
                            &api,
                            serde_json::from_str(&line).map_err(io)?,
                        )
                    })();
                    let wire = result.unwrap_or_else(|e| json!({"error":e}));
                    let _ = writeln!(stream, "{wire}");
                }
            })
        })
        .map_err(io)?;
    started.recv().map_err(io)??;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, record.to_string()).map_err(io)?;
    std::fs::rename(&temporary, &path).map_err(io)?;
    serving.insert(path);
    Ok(())
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
