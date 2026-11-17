#!/usr/bin/env python3
"""Fail closed unless a trusted real-KVM runner is online."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


REQUIRED_LABELS = {"self-hosted", "linux", "x64", "computer-use-kvm"}


def online_trusted_runner_count(payload: object) -> int:
    """Count online runners with every required label across paginated output."""
    if not isinstance(payload, list):
        raise ValueError("runner API payload must contain the paginated response list")
    count = 0
    for page in payload:
        if not isinstance(page, dict) or not isinstance(page.get("runners"), list):
            raise ValueError("runner API page has no runner list")
        for runner in page["runners"]:
            if not isinstance(runner, dict):
                raise ValueError("runner API returned a malformed runner entry")
            labels = runner.get("labels")
            if not isinstance(labels, list):
                continue
            label_names = {
                label["name"]
                for label in labels
                if isinstance(label, dict) and isinstance(label.get("name"), str)
            }
            if runner.get("status") == "online" and REQUIRED_LABELS <= label_names:
                count += 1
    return count


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("runner_json", type=Path)
    parser.add_argument("--github-output", type=Path, required=True)
    parser.add_argument("--github-summary", type=Path, required=True)
    args = parser.parse_args()
    try:
        payload = json.loads(args.runner_json.read_text(encoding="utf-8"))
        count = online_trusted_runner_count(payload)
        if count < 1:
            raise ValueError(
                "No online self-hosted Linux x64 runner labeled computer-use-kvm is registered. "
                "Register a trusted, isolated runner before requiring R1 real-host acceptance."
            )
        with args.github_output.open("a", encoding="utf-8") as output:
            output.write("available=true\n")
        with args.github_summary.open("a", encoding="utf-8") as summary:
            summary.write(
                f"Found {count} online runner(s) with the complete trusted KVM label set.\n"
            )
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
