#!/usr/bin/env python3
"""Summarize a nextest JUnit report and its measured wall time for CI."""

from __future__ import annotations

import os
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


def test_cases(root: ET.Element) -> list[ET.Element]:
    return list(root.iter("testcase"))


def summary(report: Path, timing: Path) -> str:
    cases = test_cases(ET.parse(report).getroot())
    if not cases:
        raise ValueError(f"JUnit report contains no test cases: {report}")

    skipped = [case for case in cases if case.find("skipped") is not None]
    failed = [
        case
        for case in cases
        if case.find("failure") is not None or case.find("error") is not None
    ]
    executed = len(cases) - len(skipped)
    passed = executed - len(failed)

    real_seconds = next(
        (line.split()[1] for line in timing.read_text().splitlines() if line.startswith("real ")),
        None,
    )
    if real_seconds is None:
        raise ValueError(f"No real wall-time entry in {timing}")

    lines = [
        "## Rust test suite timing",
        "",
        "| Run | Wall time | Executed | Passed | Failed | Skipped |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
        f"| {os.environ.get('KOOLADE_TEST_SUITE_LABEL', 'Nextest selection')} | {real_seconds}s | {executed} | {passed} | {len(failed)} | {len(skipped)} |",
        "",
        "This reports the exact suite and outcomes for this run. Compare only equivalent suites and CI environments.",
        "",
        "### 20 slowest test cases",
        "",
        "| Test | Duration |",
        "| --- | ---: |",
    ]
    slowest = sorted(
        (case for case in cases if case.find("skipped") is None),
        key=lambda case: float(case.get("time", "0")),
        reverse=True,
    )[:20]
    for case in slowest:
        name = case.get("name", "unknown")
        classname = case.get("classname")
        full_name = f"{classname}::{name}" if classname else name
        full_name = full_name.replace("|", "\\|")
        lines.append(f"| `{full_name}` | {float(case.get('time', '0')):.3f}s |")

    return "\n".join(lines) + "\n"


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <time-file> <junit-xml>", file=sys.stderr)
        return 2
    try:
        print(summary(Path(sys.argv[2]), Path(sys.argv[1])), end="")
    except (OSError, ET.ParseError, ValueError) as error:
        print(f"Could not summarize nextest results: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
