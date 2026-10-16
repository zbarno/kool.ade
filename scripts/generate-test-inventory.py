#!/usr/bin/env python3
"""Join nextest's test list and JUnit report with Rust test source locations."""

import argparse
import csv
import json
import pathlib
import re
import xml.etree.ElementTree as ET


def test_sources(repo):
    index = {}
    files = list((repo / "src").rglob("*.rs")) + list((repo / "tests").rglob("*.rs"))
    for path in files:
        source = path.read_text(errors="replace")
        for match in re.finditer(
            r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^>]*>)?\s*\(", source
        ):
            index.setdefault(match.group(1), []).append(
                (path.relative_to(repo).as_posix(), source.count("\n", 0, match.start()) + 1)
            )
    return index


def module_parts(source):
    return (
        source.removeprefix("src/")
        .removeprefix("tests/")
        .removesuffix(".rs")
        .split("/")
    )


def ordered_subsequence(parts, modules):
    cursor = 0
    for part in parts:
        try:
            cursor = modules.index(part, cursor) + 1
        except ValueError:
            return False
    return True


def source_for(test_id, index):
    modules = test_id.rsplit("::", 1)[0].split("::")
    candidates = index.get(test_id.rsplit("::", 1)[-1], [])
    exact = [candidate for candidate in candidates if ordered_subsequence(module_parts(candidate[0]), modules)]
    if exact:
        selected = max(exact, key=lambda item: (len(item[0]), item[0]))
        mapping = "module path match"
    elif len(candidates) == 1:
        selected = candidates[0]
        mapping = "unique function declaration"
    elif candidates:
        selected = max(
            candidates,
            key=lambda item: (
                sum(part in modules for part in module_parts(item[0])),
                len(item[0]),
                item[0],
            ),
        )
        mapping = "function-name fallback; inspect source"
    else:
        raise ValueError(f"no Rust source function found for {test_id}")
    return f"{selected[0]}:{selected[1]}", mapping


def signals(test_id, source):
    haystack = f"{test_id} {source}".lower()
    categories = []
    if any(term in haystack for term in ("end_to_end", "integration", "docker", "remote", "sshd", "artifact_layout_regression", "multi_repository_feature")):
        categories.append("E2E/integration")
    if any(term in haystack for term in ("authorize", "authorization", "policy", "sandbox", "security", "revok", "permission", "credential")):
        categories.append("security")
    if any(term in haystack for term in ("recovery", "resume", "retry", "restart", "replay", "restore", "migration")):
        categories.append("recovery/migration")
    if "src/ui/" in haystack or "src/app/" in haystack:
        categories.append("UI/application")
    if not categories:
        categories.append("unit/component")

    dependencies = []
    for terms, label in (
        (("docker",), "Docker"),
        (("remote", "ssh", "sshd"), "SSH/remote worker"),
        (("git", "worktree", "repository", "clone"), "Git/repository"),
    ):
        if any(term in haystack for term in terms):
            dependencies.append(label)
    return "; ".join(categories), "; ".join(dependencies) or "not indicated by name/path"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--nextest-json", type=pathlib.Path, required=True)
    parser.add_argument("--junit", type=pathlib.Path, required=True)
    parser.add_argument("--repo", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()

    listing = json.loads(args.nextest_json.read_text())["rust-suites"]
    junit = ET.parse(args.junit).getroot()
    timing = {}
    outcome = {}
    for case in junit.iter("testcase"):
        case_name = case.attrib["name"]
        classname = case.attrib.get("classname", "")
        # The primary library suite emits globally qualified test paths as the
        # testcase name. Auxiliary integration targets need their target name
        # prepended because their testcase names are local function names.
        key = case_name if classname == "koolade" else f"{classname}::{case_name}"
        timing[key] = float(case.attrib.get("time", "0"))
        outcome[key] = "skipped" if case.find("skipped") is not None else "passed"

    sources = test_sources(args.repo)
    entries = []
    for suite, data in listing.items():
        for test, details in data["testcases"].items():
            if details.get("kind") != "test":
                continue
            # The primary `koolade` library suite already emits fully qualified
            # Rust test paths. Prefix only auxiliary integration-test targets,
            # whose test function names are not globally unique.
            test_id = test if suite == "koolade" else f"{suite}::{test}"
            entries.append((suite, test, test_id))

    same_name = {}
    for _, test, test_id in entries:
        same_name.setdefault(test.rsplit("::", 1)[-1], []).append(test_id)

    rows = []
    for _, test, test_id in entries:
        source, source_mapping = source_for(test_id, sources)
        category, dependencies = signals(test_id, source)
        candidates = [candidate for candidate in same_name[test.rsplit("::", 1)[-1]] if candidate != test_id]
        rows.append(
            [
                test_id,
                source,
                source_mapping,
                test.rsplit("::", 1)[-1].replace("_", " "),
                category,
                f"{timing.get(test_id, 0):.3f}",
                outcome.get(test_id, "not found in JUnit"),
                dependencies,
                "; ".join(candidates) if candidates else "no exact test-name collision",
                "Review same-name candidate" if candidates else "No name collision; semantic review pending",
                "Name/source/duration inventory only; name collision is not proof of duplication",
            ]
        )

    reviewed = {}
    if args.output.is_file():
        with args.output.open(newline="") as previous:
            for row in csv.DictReader(previous):
                if row.get("evidence / review status") != "Name/source/duration inventory only; name collision is not proof of duplication":
                    reviewed[row["test"]] = (row["decision"], row["evidence / review status"])
    for row in rows:
        if row[0] in reviewed:
            row[9], row[10] = reviewed[row[0]]

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(
            [
                "test",
                "source",
                "source mapping",
                "intent summary (test name)",
                "category signals",
                "duration seconds",
                "CI outcome",
                "dependency signals",
                "overlap candidate",
                "decision",
                "evidence / review status",
            ]
        )
        writer.writerows(sorted(rows, key=lambda row: row[0]))
    print(f"wrote {len(rows)} tests to {args.output}")


if __name__ == "__main__":
    main()
