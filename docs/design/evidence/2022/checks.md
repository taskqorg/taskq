# #2022 verification

Applied rules: R9 preserves operator authority; R11 preserves active attempts; R12 separates native observations from fixture/source checks; R13 permits correcting the implementation placeholder without changing a decided rule. No R-number is amended.

## Failure before and pass after

```text
cargo test --locked --bin taskq owner_transcripts_use_native_homes -- --nocapture
BEFORE: exit 101; one failed test; Windows transcript paths are not qualified.
AFTER: exit 0; one passed test; 0.07 s test execution, 47.66 s incremental build.
```

The unchanged check runs child processes with Windows `USERPROFILE` and no `HOME`, then with explicit runtime-home overrides. It retains malformed-ID, missing-transcript, ambiguous-identity and unchanged-byte assertions. The initial cold dependency build is not a CI timing qualification.

Runtime implementation tested by the native instrument, UTF-8/LF SHA-256 of `src/runtime/owner.rs`: `b8016628e671d38dae529b00a3df3e0877b80798599680df3f8323b31c0f7e6c`. Native observations used the pinned base plus this diff. The final canonical gate separately verifies the rebased candidate; no live installation is performed.

## Native and component boundaries

- Service-owned Claude chat: exact first/second/third replies, same session, provider Read of the owned file, successful completed turn, concurrent-turn refusal and immutable launch readback.
- Production draft APIs on disposable records: stale-save refusal, reply readback, archive/restore and unsent-draft/file preservation. Existing saved-view keys compare equal.
- Native timestamp provenance: three adapter values equal the source JSONL timestamps at their byte cursors.
- Codex: this worker's real transcript lookup and bounded parse only; no owner launch or PM exchange.
- Disposable #2029: held/readback/Start, service worker, clarification, prescribed answer from the native PM chat, no-change worker result, reviewer verdict and service close. No agent closed/relabelled it.
- `node tests/relative-time.test.cjs`, `node tests/pm-composer.test.cjs`, `node tests/reply-files.test.cjs`, `node tests/task-workspace.test.cjs`: PASS after `npm.cmd run build`; fixtures/component provenance only.
- Initial standalone probe: missing WebView2 loader caused process startup refusal before a setup record existed. Cargo then ran the actual instrument; the preserved instrument uses its matching compiled loader. No native request was replayed.
- Initial `python` launcher did not run; the installed `python3.12.exe` reports Python 3.12.10 and runs the instruments. No interpreter was installed and no runtime permission changed.

The first worker-history instrument used requested spawn identity `054032c3-226f-49bf-a2dd-c53a35ac4dbe` and reported unavailable throughout; exit 0 did not prove a read. The exact attempt log's native `thread.started` identity is `74e7a5a3-bdda-4927-af6c-9fce09ecbca4`. The corrected existing `dialogue::tests::live_worker_stream_probe` read 11 native history items (seven tool-name items, one brief, one owner answer and two assistant items), including question and result hand-ins; exit 0, 66.57 s. This is a retrospective read of the disposable worker, not a measured live viewport or arrival-latency result.

## Canonical candidate gate

```text
python3.12.exe scripts/check.py --base 7902d568c0e91b5245eb0f34f15ffc7e91fa7e24
Candidate: 9ca1a0d558a0c88d1e07683a1c02e29c820ffc89
FAIL: Selected gate elapsed 925.813s / 900s; no retries.
The outer log-preservation wrapper then timed out at 930 seconds.
```

Selector/isolation/approval checks and the rule-change gate passed; no rule changed. Local contract lint was unknown because the pinned cache contains no lint script. `npm ci` completed; `npm run build` eventually completed after a long stall. The gate started `cargo +stable test --locked` but recorded no Rust completion. Canonical Rust, offline selftest and the remaining gate checks are unverified; the separately listed focused/native/component results are not a substitute.

Several independent PowerShell invocations also stopped producing output at startup, while a minimal `cmd.exe` invocation completed immediately. This is a host observation, not a demonstrated cause of the gate timeout or a repository defect. No service/policy change, budget extension, assertion weakening or blind gate retry was made. The full raw log is retained in ignored attempt scratch. Final evidence-only edits do not change the tested runtime source hash.

Publication receipts are recorded on #2022 and its PR. Merge-result CI is required for publication, not inferred from the local gate attempt (R12, §7).
