import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-computer-use-r1-kvm-runners.py"
SPEC = importlib.util.spec_from_file_location("computer_use_r1_kvm_runners", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
POLICY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POLICY)


def runner(status: str, *labels: str) -> dict[str, object]:
    return {"status": status, "labels": [{"name": label} for label in labels]}


class ComputerUseR1KvmRunnerPolicyTests(unittest.TestCase):
    def test_counts_online_trusted_runner_across_pages(self):
        payload = [
            {"runners": [runner("offline", *POLICY.REQUIRED_LABELS)]},
            {"runners": [runner("online", *POLICY.REQUIRED_LABELS)]},
        ]
        self.assertEqual(POLICY.online_trusted_runner_count(payload), 1)

    def test_rejects_runner_missing_any_trusted_label(self):
        labels = sorted(POLICY.REQUIRED_LABELS - {"computer-use-kvm"})
        payload = [{"runners": [runner("online", *labels)]}]
        self.assertEqual(POLICY.online_trusted_runner_count(payload), 0)

    def test_rejects_empty_or_malformed_paginated_payloads(self):
        self.assertEqual(POLICY.online_trusted_runner_count([]), 0)
        for payload in ({}, [{"runners": None}]):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                POLICY.online_trusted_runner_count(payload)

    def test_absent_runner_fails_without_writing_available_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runner_json = root / "runners.json"
            output = root / "github-output"
            summary = root / "github-summary"
            runner_json.write_text(json.dumps([{"runners": []}]), encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    str(runner_json),
                    "--github-output",
                    str(output),
                    "--github-summary",
                    str(summary),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("No online self-hosted Linux x64 runner", result.stderr)
            self.assertFalse(output.exists())
            self.assertFalse(summary.exists())


if __name__ == "__main__":
    unittest.main()
