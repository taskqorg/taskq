#!/usr/bin/env python3
"""R12 / #2094: unavailable native inventory must preserve the fixture, never prove exit."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

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
