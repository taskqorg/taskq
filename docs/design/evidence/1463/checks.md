# Canonical gate failure inventory

## Mac continuation on current main

`python3 scripts/check.py --base c34717e0319fd850929ed31ec0c8934af3a04782`
failed: 434 passed, 18 failed, 34 ignored; 694.177 s / 900 s, cold Cargo cache.
Explicit worker host-policy/lifecycle refusals are sandbox-blocked. Other
assertions remain unqualified; this is not baseline reproduction or a green
gate. No assertion, worker environment or authority was changed. The runner
stopped before integration/selftest/Node stages. Separate component, shell,
view-state, text, offline selftest and manifest/release checks pass.

- `host::installation::tests::detached_install_observers_share_one_acknowledgement_and_cannot_create_a_command`
- `host::installation::tests::legacy_cursor_requires_matching_drain_and_exit_proof`
- `host::tests::acknowledged_policy_survives_writer_termination`
- `host::tests::capacity_receipts_follow_the_loaded_command_not_equal_values`
- `host::tests::host_install_refuses_unresolved_legacy_owner_without_effects`
- `host::tests::local_capacity_crashes_never_replay_an_uncertain_write`
- `host::tests::local_capacity_refuses_within_bound_while_host_lock_is_held`
- `host::tests::local_capacity_request_replays_without_mutation_and_refuses_foreign_scope`
- `host::tests::local_capacity_saves_while_telemetry_is_stalled`
- `lifecycle::tests::project_uninstall_drains_before_registry_removal_and_replays_exact_receipt`
- `lifecycle::tests::project_uninstall_refuses_foreign_scope_without_registry_effect`
- `lifecycle::tests::restart_pending_survives_retention_and_receipt_failure`
- `lifecycle::tests::restart_refuses_unqualified_or_unrecordable_requests`
- `lifecycle::tests::restart_replay_scope_and_exact_generation`
- `lifecycle::tests::stop_keeps_original_receipts_and_requires_attributable_native_exit`
- `resources::tests::local_capacity_receipts_never_enter_the_remote_exchange`
- `resources::tests::project_overlay_writes_only_its_scope_and_applies_on_exact_readback`
- `tray::tests::blocked_capacity_does_not_block_pause`

## Historical Windows checks

Resumed full gate at `a2ed8b6`: `python scripts/check.py --base 00a8a4f`
failed with 424 passed, 21 failed, 35 ignored (84.640 s / 900 s). The unchanged
R8 size check passes. This retains the 20 failures below and adds
`runtime::app_control::windows_pipe::tests::native_pipe_bounds_unread_writes_and_empty_reads`:
the peer's `listener.accept()` reached `Codex App IPC deadline; cancellation
outcome unknown`, then the join failed. This timing failure remains unqualified.
The publication issue-read assertion still observes 2 instead of 0 on the other
host. Explicit host/policy/security refusals remain sandbox-blocked; neither
they nor the other assertions are PASS. The worker authority environment stays
intact.

After the notification-label repair at `a200eca`, the affected web gate
`python scripts/check.py --base a2ed8b6` passes (65.875 s / 900 s). It includes
the nine core checks, all Node components, manifest/release scripts and the
shell browser regression. The full Rust suite was not repeated for the CSS and
browser-only repair; no Rust behavior changed. This selected pass does not
erase the full-gate failures. Offline selftest passes 92 traces, formats and
staged telemetry; board unknown. Native and merge-result CI remain separate.

Owner-revision gate: `python scripts/check.py --base a4605ba` failed with
421 passed, 21 failed, 35 ignored (91.047 s / 900 s). Its extra failure was
`core::tests::every_table_row_has_a_golden_trace`: contract over 60 KiB after
the intake rebase. After shortening the Product pointer to 61,433 bytes /
298 lines, all nine core checks pass. The remaining 20 failures below remain
unresolved; the full gate was not rerun.

`python scripts/check.py --base 58744e1`: Rust 421 passed, 20 failed, 35 ignored; 50.375 s / 900 s.

The pre-rebase run against `0804397` had the same 20 failures (53.859 s).

Explicit worker/security refusals are sandbox-blocked; other assertion failures remain unqualified. No assertion or authority environment was relaxed. This is not a green gate.

- `host::installation::tests::detached_install_observers_share_one_acknowledgement_and_cannot_create_a_command`
- `host::installation::tests::legacy_cursor_requires_matching_drain_and_exit_proof`
- `host::tests::acknowledged_policy_survives_writer_termination`
- `host::tests::capacity_receipts_follow_the_loaded_command_not_equal_values`
- `host::tests::host_install_refuses_unresolved_legacy_owner_without_effects`
- `host::tests::local_capacity_crashes_never_replay_an_uncertain_write`
- `host::tests::local_capacity_refuses_within_bound_while_host_lock_is_held`
- `host::tests::local_capacity_request_replays_without_mutation_and_refuses_foreign_scope`
- `host::tests::local_capacity_saves_while_telemetry_is_stalled`
- `lifecycle::tests::project_uninstall_drains_before_registry_removal_and_replays_exact_receipt`
- `lifecycle::tests::project_uninstall_refuses_foreign_scope_without_registry_effect`
- `lifecycle::tests::restart_pending_survives_retention_and_receipt_failure`
- `lifecycle::tests::restart_refuses_unqualified_or_unrecordable_requests`
- `lifecycle::tests::restart_replay_scope_and_exact_generation`
- `lifecycle::tests::stop_keeps_original_receipts_and_requires_attributable_native_exit`
- `resources::tests::local_capacity_receipts_never_enter_the_remote_exchange`
- `resources::tests::project_overlay_writes_only_its_scope_and_applies_on_exact_readback`
- `service::tests::a_waiting_publication_reads_nothing_twice_and_only_its_host_polls`
- `tray::tests::blocked_capacity_does_not_block_pause`
- `windows_sandbox::preparation::tests::native_preparation_and_restore_preserve_controls_and_data`
