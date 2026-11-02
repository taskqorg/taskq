# Claude default-home environment (#2166)

An implicit runtime default must preserve the absence of its home override. Login
probes and native launches must agree without changing authentication or grants.
The owner request and acceptance are [#2166](https://github.com/taskqorg/taskq/issues/2166).

## Applied contract

R13: clarified the linked §3 Runtime accounts detail before implementation; the
existing root decision (runtime own default; default keeps service value) is
unchanged. R8 keeps the linked capacity detail consistent. R9: no login, credential/config copy, account migration,
provider, model, grant or capacity changes. R12 separates adapter regressions from
native service admission and merge-result CI.

## Failure before

On macOS, 2026-11-02, the same Claude 2.1.316 executable had SHA-256
`64c327e92b2c5e0f78c0a980c5a518121e1f0d140ca771ca5dd4488d1a2dcb3f`.
Only the three non-secret status fields below were retained; no credentials or
account identity were inspected or copied.

| Environment | Exit | loggedIn | authMethod | subscriptionType |
| --- | --- | --- | --- | --- |
| `CLAUDE_CONFIG_DIR` unset | 0 | true | claude.ai | max |
| Forced ordinary default directory | 1 | false | none | null |

The command was `claude auth status --json` in both cases. Installed `taskq agents`
also reported `claude 2.1.316 not logged in; kind none`. The live service's status
at epoch `1793607283`, PID `22017`, repeated the Claude login blocker despite zero
Claude slots used and an open load gate.

Before the implementation fix, the new persistent regression failed:

```text
cargo test --locked --bin taskq claude_account_environment_parity
service readiness: implicit
left: "not logged in"
right: "logged in"
test result: FAILED. 0 passed; 1 failed
```

The initial build required the existing `npm ci --ignore-scripts --no-audit --no-fund`
setup: the checkout lacked Prism modules. No dependency was added or changed.

## Fix and retained protection

The shared account environment resolver leaves the ordinary default implicit when
its home variable is absent, preserves explicit overrides verbatim, and sets named
homes explicitly. Readiness and agent rows use it. Physical transcript/account
lookup still has its directory; it does not force a login environment. Agent rows
also retain an explicitly inherited named-home path instead of stripping it back
to the parent default.

The persistent isolated-subprocess regression checks service readiness, agent rows
and AgentQ launch/resume environment parity for implicit default, explicitly set
ordinary default, custom override, inherited named override, multi-account choice
and malformed native status. Empty explicit homes cannot borrow the fallback's
invented login. Named/custom status changes are read only in their exact home.
Policy changes retain a resumed attempt's original account, native session,
input file and pending input-binding bytes. Attempt-bound transcript lookups stay
in that account; global lookup remains only for display discovery. A duplicate
session in the default home cannot supply a named attempt's background state or
saved permissions.

After the fix, the same command passed: `1 passed; 0 failed` (six isolated
environment scenarios including duplicate-session isolation, 1.16 seconds execution
on this Mac; warm build 21.72 seconds).

The regression uses invented status responses. It does not qualify native auth,
background launch, sandbox, native input acceptance, tool execution or other OSes.

A read-only independent source review found that attempt-bound transcript reads
still searched every account. The expanded regression reproduced the defect with
one default-home background marker and no named-home transcript:

```text
missing named history cannot borrow default background state
test result: FAILED. 0 passed; 1 failed
```

Resume permissions, turn observation and background fallback now bind the exact
attempt/supplied home. Default-only, duplicate-session and named-only markers are
covered; display-only session discovery retains its global lookup.

## Candidate gate

The first canonical invocation stopped before running the suite: its case-sensitive
rule-ID matcher did not recognize the task's existing `section 3 runtime accounts`
spelling as `§3 Runtime accounts`. The redundant root-contract amendment was
removed; the existing default-home decision already requires this fix. The linked
detail clarifies that decision. No rule matcher or task authority was changed.
The gate is rerun on the corrected candidate and its added account-isolation check.

## Remaining native acceptance

The existing owner-authorized capacity UI repair
[#2167](https://github.com/taskqorg/taskq/issues/2167) is the sole new-task admission
witness (inbox note `6572256310`). No placeholder or second probe task was created.
A fresh fixed-artifact service readback must identify its original task, attempt,
runtime, accepted native input and actual tool result. Clearing the login blocker
alone is not A4 evidence. Existing attempts and their sessions must remain intact.

Canonical gate, independent exact-source review, fixed-artifact native readback
and green merge-result CI are recorded separately as they become available.
