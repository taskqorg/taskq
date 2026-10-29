//! #1376: one integration binary: one link and one parallel harness instead of six sequential binaries.
mod host_policy;
mod host_restart;
mod macos_runner;
mod macos_tcc_evidence;
mod resource_policy_contract;
mod service_log;
mod telemetry_process;

// #1566: an unbounded `output()` let one stuck Windows child run the whole gate into its 900 s
// budget with nothing captured. Output goes to files, so only the child's own exit ends the wait,
// and a timeout fails this test alone with what the child wrote so far.
pub(crate) trait Bounded {fn bounded(&mut self)->std::process::Output;}
impl Bounded for std::process::Command {
    fn bounded(&mut self)->std::process::Output {
        use std::{sync::atomic::{AtomicUsize,Ordering},time::{Duration,Instant}};
        static NEXT:AtomicUsize=AtomicUsize::new(0);
        let dir=std::env::temp_dir().join(format!("qo-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let (out,err)=(dir.join("stdout"),dir.join("stderr"));
        let began=Instant::now();
        let mut child=self.stdin(std::process::Stdio::null()).stdout(std::fs::File::create(&out).unwrap()).stderr(std::fs::File::create(&err).unwrap()).spawn().unwrap();
        let status=loop {
            if let Some(status)=child.try_wait().unwrap() {break status;}
            if began.elapsed()>Duration::from_secs(60) {
                let _=child.kill();let _=child.wait();
                panic!("{:?} still running after {:?}; stderr:\n{}",self,began.elapsed(),std::fs::read_to_string(&err).unwrap_or_default());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let output=std::process::Output {status,stdout:std::fs::read(&out).unwrap(),stderr:std::fs::read(&err).unwrap()};
        let _=std::fs::remove_dir_all(dir);output
    }
}
