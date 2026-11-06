//! R1/R4/R12: credential-authenticated, read-only local service transport.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{Mutex, OnceLock},
};

const WIRE: u64 = 1;

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
fn request(api: &Api, value_of_request: Value) -> Option<Result<Value>> {
    if !preferred() || crate::host::active() {
        return None;
    }
    api.proxy_listed.set(None);
    if !api.local_reader {
        disclose("local sharing unsupported for this board");
        return None;
    }
    let Some(install) = api.install.as_ref() else {
        disclose("local installation unavailable");
        return None;
    };
    let result = (|| -> Result<Value> {
        let status = crate::host::service_status(install);
        if status["state"] != "running" || status["fresh"] != true {
            return Err("local service absent, stale or ownership unknown".into());
        }
        let value = crate::data::json(&endpoint(install, &api.base))?
            .ok_or("local service has no board reader (older service or startup failure)")?;
        if value["schema"] != 1 || value["wire"] != WIRE {
            return Err("local board reader wire incompatible".into());
        }
        if value["board"] != api.base
            || value["pid"] != status["pid"]
            || value["started"] != status["started"]
        {
            return Err("local board reader owner differs from the running service".into());
        }
        let address = value["address"]
            .as_str()
            .ok_or("local board reader address missing")?
            .parse::<SocketAddr>()
            .map_err(io)?;
        if !address.ip().is_loopback() {
            return Err("local board reader is not loopback".into());
        }
        api.refresh()?;
        let mut stream = TcpStream::connect_timeout(&address, HTTP_WAIT).map_err(io)?;
        connection(&stream)?;
        let payload = value_of_request.to_string();
        let wire =
            json!({"wire":WIRE,"request":payload,"proof":sign(api, payload.as_bytes()).as_ref()});
        writeln!(stream, "{wire}").map_err(io)?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).map_err(io)?;
        let reply: Value = serde_json::from_str(&line).map_err(io)?;
        if reply["wire"] != WIRE {
            return Err("local board reply wire incompatible".into());
        }
        if let Some(reason) = reply["unavailable"].as_str() {
            return Err(format!("local board reader unavailable: {reason}").into());
        }
        if !reply["error"].is_null() {
            return Ok(reply);
        }
        api.proxy_listed.set(reply["listed"].as_i64());
        let mut budget = api.credentials.borrow().budget.borrow_mut().clone();
        budget.remaining = reply["budget"]["remaining"].as_u64();
        budget.limit = reply["budget"]["limit"].as_u64();
        budget.reset = reply["budget"]["reset"].as_i64();
        budget.resource = reply["budget"]["resource"].as_str().map(String::from);
        budget.unknown = reply["budget"]["unknown"].as_str().map(String::from);
        *api.credentials.borrow().budget.borrow_mut() = budget;
        *api.clock.borrow_mut() = reply["clock"].as_i64().ok_or("local board clock missing")?;
        api.unchanged.set(
            reply["unchanged"]
                .as_bool()
                .ok_or("local board read status missing")?,
        );
        Ok(reply)
    })();
    match result {
        Ok(reply) if !reply["error"].is_null() => Some(Err(serde_json::from_value(
            reply["error"].clone(),
        )
        .unwrap_or_else(io))),
        Ok(reply) => Some(Ok(reply)),
        Err(error) => {
            disclose(&error.text());
            None
        }
    }
}
pub(super) fn disclose(reason: &str) {
    thread_local! { static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) }; }
    LAST.with(|last| {
        if *last.borrow() != reason {
            eprintln!("Board reads: direct ({reason})");
            *last.borrow_mut() = reason.to_string();
        }
    });
}

pub(super) fn raw(
    api: &Api,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Option<Result<Reply>> {
    if method != "GET" || body.is_some() {
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

fn answer(api: &Api, wire: Value) -> Result<Value> {
    if wire["wire"] != WIRE {
        return Err("local board request wire incompatible".into());
    }
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
            json!({"issues":super::shared::snapshot(api, since, tasks)?,"listed":api.proxy_listed.get()})
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
            match api.call(method, url, body.cloned()) {
                Ok(reply) => json!({"reply":reply}),
                Err(error) => json!({"error":error}),
            }
        }
        _ => return Err("local board request kind invalid".into()),
    };
    result["wire"] = json!(WIRE);
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
    if api.poll.is_none() || !api.local_reader {
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
    let root = crate::paths::find_root()?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(io)?;
    let owner = coreq::process::identity(std::process::id())
        .map_err(io)?
        .ok_or("local board owner identity unknown")?;
    let record = json!({"schema":1,"wire":WIRE,"board":api.base,"pid":owner.pid,"started":owner.started,"address":listener.local_addr().map_err(io)?.to_string()});
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, record.to_string()).map_err(io)?;
    std::fs::rename(&temporary, &path).map_err(io)?;
    let (name, host, repo, tool) = (
        name.to_string(),
        host.to_string(),
        repo.to_string(),
        tool.clone(),
    );
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let started = std::thread::Builder::new()
        .name("taskq-board-reader".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(error) => {
                        crate::platform::log(&format!("local board listener stopped: {error}"));
                        break;
                    }
                };
                let (name, host, repo, tool, root, dispatch) = (
                    name.clone(),
                    host.clone(),
                    repo.clone(),
                    tool.clone(),
                    root.clone(),
                    dispatch.clone(),
                );
                if let Err(error) = std::thread::Builder::new()
                    .name("taskq-board-read".into())
                    .spawn(move || {
                        tracing::dispatcher::with_default(&dispatch, || {
                            crate::host::enter(&root, &tool.install);
                            let api =
                                super::kind(&name).and_then(|kind| kind.http(&host, &repo, &tool));
                            if let Err(error) = serve(stream, api) {
                                crate::platform::log(&error.text());
                            }
                        });
                    })
                {
                    crate::platform::log(&format!("local board connection unavailable: {error}"));
                }
            }
        });
    if let Err(error) = started {
        let _ = std::fs::remove_file(&path);
        return Err(io(error));
    }
    serving.insert(path);
    Ok(())
}
fn serve(mut stream: TcpStream, api: Result<Api>) -> Result<()> {
    connection(&stream)?;
    let result = (|| {
        let mut line = String::new();
        BufReader::new(stream.try_clone().map_err(io)?)
            .read_line(&mut line)
            .map_err(io)?;
        answer(&api?, serde_json::from_str(&line).map_err(io)?)
    })();
    let reply =
        result.unwrap_or_else(|error: Error| json!({"wire":WIRE,"unavailable":error.text()}));
    writeln!(stream, "{reply}").map_err(io)
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
