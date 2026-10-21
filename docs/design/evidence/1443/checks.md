# Local canonical gate - #1443

Candidate product source: `8bf66914f03560c8c3f979523dce7d6781fe5420`; only research evidence/instrument files added.

Command: `python3 scripts/check.py --base 8bf66914f03560c8c3f979523dce7d6781fe5420`.

Full selection: Rust 427 passed, 21 failed, 36 ignored. Gate elapsed 322.562 s / 900 s, no retries. The failure stopped the later offline selftest and Node gate stage. Separate focused browser outcomes are in [the report](README.md#checks-and-remaining-work).

The worker environment was preserved. Host authorization refusals are `sandbox-blocked` for acceptance; no unrestricted rerun was attempted. Do not attribute all 21 failures to those refusals.

Observed diagnostics include:

- `a worker cannot install the host service`
- `a worker cannot change host policy`
- `a worker cannot restart the host service or request Stop`
- `Windows security restore requires an unrestricted operator`
- Windows error 32: `The process cannot access the file because it is being used by another process.`
- Publication read counts differed; crash/commit/replay assertions also failed. Their cause is unknown in this review.

Failed tests (verbatim identifiers):

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
- `platform::tests::short_dependency_outputs_are_bounded_and_preserve_nonzero_receipts`
- `resources::tests::local_capacity_receipts_never_enter_the_remote_exchange`
- `resources::tests::project_overlay_writes_only_its_scope_and_applies_on_exact_readback`
- `service::tests::a_waiting_publication_reads_nothing_twice_and_only_its_host_polls`
- `tray::tests::blocked_capacity_does_not_block_pause`
- `windows_sandbox::preparation::tests::native_preparation_and_restore_preserve_controls_and_data`

These failures are recorded, not fixed or waived. No after-pass, full-gate success, native qualification or merge-result CI success is claimed.
