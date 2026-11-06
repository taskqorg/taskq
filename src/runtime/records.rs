//! AgentQ owns records; telemetry locks and transcript parsing never run on supervision.
use std::{
    path::Path,
    sync::{OnceLock, mpsc},
};

type Job = Box<dyn FnOnce() + Send>;

static QUEUE: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

// ponytail: one FIFO worker, as for command receipts; a job still queued at a hard exit stays unknown.
fn queue() -> &'static mpsc::Sender<Job> {
    QUEUE.get_or_init(|| {
        let (send, jobs) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("taskq-agent-records".into())
            .spawn(move || {
                for job in jobs {
                    job();
                }
            })
            .expect("agent record worker");
        send
    })
}

pub(super) fn submit(
    install: &Path,
    record: impl FnOnce(&coreq::telemetry::Store) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let install = install.to_owned();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let caller = coreq::telemetry::capture::TraceContext::current();
    queue()
        .send(Box::new(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                let _caller = caller.as_ref().map(|c| c.span().entered());
                let result = crate::telemetry::budget(&install, "telemetry.sqlite3")
                    .and_then(|budget| {
                        coreq::telemetry::Store::open_bounded(
                            &install.join("telemetry.log/universal.sqlite3"),
                            budget,
                        )
                    })
                    .and_then(|store| record(&store));
                if let Err(error) = result {
                    eprintln!("Agent telemetry: unknown ({error})");
                }
            });
        }))
        .map_err(|_| "agent record worker unavailable".into())
}

/// Waits until submitted records have committed or failed; a process that submitted none returns at once.
pub(crate) fn settle(limit: std::time::Duration) -> bool {
    let Some(queue) = QUEUE.get() else {
        return true;
    };
    let (send, done) = mpsc::channel();
    queue
        .send(Box::new(move || {
            let _ = send.send(());
        }))
        .is_ok()
        && done.recv_timeout(limit).is_ok()
}
