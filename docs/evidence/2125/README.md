# Windows project Uninstall ownership

Task: https://github.com/taskqorg/taskq/issues/2125

The defect is an unconditional OWNER/GROUP/DACL copy through `SetFileSecurityW`.
An ordinary owner with Modify access can write the registry but lacks
WRITE_OWNER. Reapplying unchanged owner/group fields therefore fails before
replacement. The fix copies only differing identity fields and retains DACL
protection and ownership readback (R9, R12; sections 8 and 9).

## Before and after

An isolated native PowerShell/PInvoke probe under the ordinary Windows token
(`Administrator=false`) created a file in a Modify-only directory. With the
same security descriptor, `SetFileSecurityW` information 7 failed with Win32
error 5; information 4 succeeded. No privilege was enabled. This is the native
API reproduction, not a baseline Rust-suite failure.

`cargo test --locked --bin taskq -- windows_acl lifecycle::tests:: lifecycle_scope project_uninstall_preserves_other_project_work_and_admission --nocapture`
passed 14 tests, with one owned subprocess helper ignored by the harness; the
parent lifecycle check invokes that helper. Runtime was 3.84 seconds after the
warm Windows build. The initial test attempt had an invalid .NET fixture enum;
the corrected fixture uses TakeOwnership for WRITE_OWNER. A subsequent fixture
attempt incorrectly rewound a public receipt projection; recovery now rewinds
the retained internal record. Neither fixture error is defect reproduction.

The native registry regression denies WRITE_OWNER while retaining Modify and
the owner's DACL-management rights. It checks inherited and protected DACLs,
exact owner/group/DACL and protection readback, refusal of differing ownership,
and bounded rejection of corrupt SID offsets. `ownership()` retains its existing
normalization of AUTO_INHERITED bookkeeping; whole-descriptor byte identity is
not claimed.

## Native receipts

The actual Windows fixture owner was PID `21648`, creation identity
`134380048947857443`, service instance `registry-fixture`. The original request
was `registry-uninstall`; its native receipt store was read back after flush.
Temporary project paths are omitted.

| DACL | Received | Applied | Observed |
|---|---|---|---|
| Inherited | `81ef1f07c5fded9ff62f317545657c8d` | `ecde3120e4d89c730897e1fc3860ae4c` | `87ea3254ad8d34e7e5203e958a2b3004` |
| Protected | `da3e727f2e80dbb8d28bcfe007d468b1` | `5d74b04311314b3ece87464833f33950` | `87b15b42f4830cf1b13d2fe2e18b4388` |

These are ordinary-token native filesystem and receipt-store observations in an
isolated service-observer fixture, not an installed Scheduler service rollout.

## Acceptance boundaries

- A1/A3: real Windows security APIs, owner/group/DACL preservation and differing
  identity refusal. The installed host, its task definition and grants were not
  changed; the PM's private installed reproduction was not rerun here.
- A2: the lifecycle regression retains an applied receipt after write refusal,
  reconciles the same ID after registry commit, and rejects changed ACLs or a
  missing original ownership snapshot before observation. The adapter regression
  injects the named project's drain and retains another project's live FakeProc
  attempt while admitting its next job. This is adapter evidence, not native
  worker survival qualification. Existing last-project Stop/Uninstall process
  checks retain native-exit and same-receipt protections.
- A4: one formatter is used for initial command output, receipt replay and service
  status. It names the removal target separately from the reviewed registry;
  host Uninstall states that every project drains while the registry is kept.

The private ownership snapshot stays in the existing applied operation record
and is removed from public receipt projections. Observation checks it before
and after registry commit. A mismatch stays applied/unknown; it is not silently
repaired and never becomes a fabricated failed or observed receipt (R12).
Older lifecycle readers refuse records carrying this field, including retained
terminal records; rollback requires a compatible reader and preserved evidence.
No root-contract decision or R-number was amended (R13).

## Merge-candidate fixture repair

[Merge-group run 41306809372](https://github.com/taskqorg/taskq/actions/runs/41306809372/job/124848843242)
tested merge SHA `3b50a8df259701c6641a615af5524aaaa33ebd2a` with PR head
`e6f50ab5f2f8951c7eebe5a7f9641b82f5520f58`. The Windows unit run had
815 passes and one failure: this registry regression stopped in fixture setup
because `Get-Acl` found `Microsoft.PowerShell.Security` but could not load it.
The log does not establish why module loading failed. Linux, macOS and the
Windows GNU loader check passed; the merge gate failed (R12, section 7).

The fixture now calls the Windows PowerShell framework's directory/file
`GetAccessControl` and `SetAccessControl` methods directly. Every ACL rule and
assertion remains: Modify with denied WRITE_OWNER, inherited/protected DACLs,
exact ownership, refused foreign ownership, uncertain-write and same-ID recovery,
receipt readback, selective drain and unrelated admission. Production code and
the contract are unchanged by this repair.

A one-time local probe ran both exact fixture scripts with `Get-Acl` and
`Set-Acl` replaced by throwing functions. Both original scripts failed; both
framework scripts passed. This proves independence from those cmdlets, not the
cause of the CI module failure. Altering only the local module search path did
not reproduce that failure. The actual before failure is the merge-group log.

After repair, the focused command above passed all 14 checks in 3.03 seconds
after a 57.44-second warm Windows build, with the same one owned helper ignored.
Fresh native observer PID `33260`, creation identity `134380104981123042`,
instance `registry-fixture`, retained these read-back receipts:

| DACL | Received | Applied | Observed |
|---|---|---|---|
| Inherited | `87641d491a2eec5022471b7a79184dff` | `05cfae0ce1192563e5bd0656a2d0520b` | `b0c09ec34ef38917bcea9508997a35a3` |
| Protected | `b22a39beb1bec71879e83756f1d95eb0` | `5e3721b1999b62ed4747a9e91a46a822` | `0e3e9b26fdd59112dcf3163a16a2a62f` |

These remain isolated ordinary-token filesystem/receipt observations. Under
owner note `6557913806`, installed Scheduler-host verification is deferred to
post-merge release; no ordinary service restart, deployment or grant was made.
