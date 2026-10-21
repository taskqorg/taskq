# Canonical gate failure inventory

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
