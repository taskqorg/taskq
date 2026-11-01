#!/usr/bin/env python3
"""R12 / #2094: unavailable native inventory must preserve the fixture, never prove exit."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import mock_open, patch

spec = importlib.util.spec_from_file_location("live_probe", Path(__file__).with_name("live-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Inventory(unittest.TestCase):
    def test_failed_process_inventory_refuses_launch_and_cleanup(self):
        for result in [(None, "unknown (Operation not permitted)"), (1, "access denied"), (0, "")]:
            with self.subTest(result=result), patch.object(probe, "run", return_value=result):
                with self.assertRaisesRegex(SystemExit, "Process inventory unknown"):
                    probe.procs({"dir": "/invented/probe-2094"})

    def test_process_inventory_keeps_foreign_processes_out(self):
        with patch.object(probe, "run", return_value=(0,
                "123 /invented/probe-2094/taskq service run\n124 /invented/ordinary/taskq service run")):
            self.assertEqual(probe.procs({"dir": "/invented/probe-2094"}),
                             [(123, "/invented/probe-2094/taskq service run")])

    def test_process_ownership_requires_folder_boundaries(self):
        for windows, folder in [(False, "/invented/probe-2094"),
                                (False, "/invented/space parent/probe-2094"),
                                (True, r"C:\Invented\Space Parent\Probe-2094")]:
            with self.subTest(windows=windows, folder=folder), patch.object(probe, "WIN", windows):
                commands = [f'"{folder}"', f'"{folder}/root/taskq" service run',
                            f'taskq --target="{folder}/install"', f'{folder}/taskq service run',
                            f'"{folder}-other/root/taskq" service run',
                            f'"{folder}.old/taskq" service run',
                            f'"prefix{folder}/taskq" service run',
                            f'"{folder} other/taskq" service run']
                with patch.object(probe, "run", return_value=(0, "\n".join(
                        f"{100 + i} {cmd}" for i, cmd in enumerate(commands)))):
                    self.assertEqual(probe.procs({"dir": folder}),
                                     [(100 + i, cmd) for i, cmd in enumerate(commands[:4])])

    def test_reconcile_keeps_sibling_process_after_excluding_own(self):
        own = "/invented/probe-2094"
        with patch.object(probe, "WIN", False), patch.object(probe.os.path, "isdir", return_value=False), \
                patch("builtins.open", mock_open(read_data="{}")), patch.object(probe, "run", return_value=(0,
                    f'123 "{own}/taskq" service run\n124 "{own}-other/taskq" service run')):
            bad = probe.reconcile("/invented", "/repo", own=own)
            self.assertEqual(len(bad), 1)
            self.assertIn("process 124 names an earlier fixture", bad[0])

    def test_failed_or_unrecognized_claude_inventory_keeps_fixture(self):
        m = {"dir": "/invented/probe-2094", "root": "/invented/probe-2094/root"}
        for result in [(None, "unknown"), (1, "access denied"), (0, "not json"), (0, "{}"),
                       (0, '{"agents":null}'), (0, '[{"id":"fake","state":"unknown"}]')]:
            with self.subTest(result=result), patch.object(probe, "run", return_value=result):
                with self.assertRaisesRegex(SystemExit, "Claude job inventory unknown"):
                    probe.jobs(m)

    def test_codex_only_fixture_never_probes_disabled_claude(self):
        with patch.object(probe, "run", side_effect=AssertionError("disabled runtime queried")):
            self.assertEqual(probe.jobs({"runtimes": ["codex"]}), [])

    def test_blocked_claude_job_is_live_and_owned(self):
        m = {"dir": "/invented/probe-2094", "root": "/invented/probe-2094/root"}
        with patch.object(probe, "run", return_value=(0,
                '[{"id":"fake-owned","state":"blocked","cwd":"/invented/probe-2094/root"},'
                '{"id":"fake-foreign","state":"idle","cwd":"/invented/ordinary"},'
                '{"id":"fake-sibling","state":"idle","cwd":"/invented/probe-2094-other/root"}]')):
            live = [r["id"] for r in probe.jobs(m) if r["state"] in probe.LIVE]
            self.assertEqual(live, ["fake-owned"])


if __name__ == "__main__":
    unittest.main()
