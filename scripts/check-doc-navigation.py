#!/usr/bin/env python3
"""Check project documentation navigation indexes without trusting their prose."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
HEADING = "# Navigation"
LINK = re.compile(r"^\s*- \[([^]]+)\]\(([^)]+)\) - (.+?)\s*$")
DOCUMENT_SUFFIXES = {".md", ".png", ".jpg", ".jpeg", ".webp", ".gif"}


def document_directories() -> list[Path]:
    directories = [ROOT]
    docs_root = ROOT / "docs"
    if docs_root.is_dir():
        directories.append(docs_root)

    def contains_document(directory: Path) -> bool:
        return any(
            path.is_file()
            and path.suffix.lower() in DOCUMENT_SUFFIXES
            and path.name != "AGENTS.md"
            for path in directory.rglob("*")
        )

    directories.extend(
        directory
        for directory in sorted(docs_root.rglob("*"))
        if directory.is_dir()
        and contains_document(directory)
    )
    return directories


def immediate_targets(directory: Path) -> list[tuple[str, Path, str]]:
    files = sorted(
        path
        for path in directory.iterdir()
        if path.is_file()
        and path.suffix.lower() in DOCUMENT_SUFFIXES
        and path.name != "AGENTS.md"
    )
    children = sorted(
        child
        for child in directory.iterdir()
        if child.is_dir()
        and (child / "AGENTS.md").is_file()
    )
    targets = []
    for path in files:
        if path.suffix.lower() == ".md":
            title = next(
                (
                    line.lstrip("# ").strip()
                    for line in path.read_text(encoding="utf-8").splitlines()
                    if line.startswith("# ")
                ),
                path.stem.replace("-", " ").replace("_", " "),
            )
        else:
            title = f"Screenshot {path.stem.replace('-', ' ').replace('_', ' ')}"
        targets.append((path.name, path, f"Read for {title}."))
    for path in children:
        title = path.name.replace("-", " ").replace("_", " ")
        targets.append((f"{path.name}/", path / "AGENTS.md", f"Index for {title} documentation."))
    return targets


def section(directory: Path) -> str:
    lines = [HEADING, ""]
    for label, target, summary in immediate_targets(directory):
        relative = target.relative_to(directory).as_posix()
        lines.append(f"- [{label}](./{relative}) - {summary}")
    return "\n".join(lines).rstrip() + "\n"


def replace_navigation(existing: str, generated: str) -> str:
    lines = existing.splitlines(keepends=True)
    start = next((i for i, line in enumerate(lines) if line.strip() == HEADING), None)
    if start is None:
        prefix = existing.rstrip()
        return (prefix + "\n\n" if prefix else "") + generated
    end = next(
        (i for i in range(start + 1, len(lines)) if re.match(r"^# (?!# )", lines[i])),
        len(lines),
    )
    before = "".join(lines[:start]).rstrip()
    after = "".join(lines[end:]).lstrip("\n")
    parts = [part for part in (before, generated.rstrip(), after.rstrip()) if part]
    return "\n\n".join(parts) + "\n"


def check(directory: Path) -> list[str]:
    index = directory / "AGENTS.md"
    if not index.is_file():
        return [f"missing {index.relative_to(ROOT)}"]
    content = index.read_text(encoding="utf-8")
    nav = content.split(HEADING, 1)[1] if HEADING in content else ""
    nav = nav.split("\n# ", 1)[0]
    actual = []
    errors = []
    for line in nav.splitlines():
        if line.strip().startswith("-"):
            match = LINK.match(line)
            if not match:
                errors.append(f"malformed navigation entry in {index.relative_to(ROOT)}: {line}")
                continue
            label, href, summary = match.groups()
            target = (directory / href).resolve()
            if not target.is_file():
                errors.append(f"broken navigation link in {index.relative_to(ROOT)}: {href}")
            if not summary.strip():
                errors.append(f"empty navigation summary in {index.relative_to(ROOT)}: {href}")
            actual.append((label, href, target))
    expected = [
        (label, f"./{target.relative_to(directory).as_posix()}", target.resolve())
        for label, target, _ in immediate_targets(directory)
    ]
    if actual != expected:
        errors.append(f"stale or incomplete immediate navigation coverage in {index.relative_to(ROOT)}")
    if len(actual) != len(set(actual)):
        errors.append(f"duplicate navigation entry in {index.relative_to(ROOT)}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="update only each Navigation section")
    args = parser.parse_args()
    directories = document_directories()
    if args.write:
        for directory in directories:
            index = directory / "AGENTS.md"
            if not index.exists():
                index.write_text("", encoding="utf-8")
        for directory in directories:
            index = directory / "AGENTS.md"
            original = index.read_text(encoding="utf-8") if index.exists() else ""
            index.write_text(replace_navigation(original, section(directory)), encoding="utf-8")
    errors = [error for directory in directories for error in check(directory)]
    if errors:
        print("\n".join(errors))
        return 1
    print(f"checked navigation indexes in {len(directories)} documentation directories")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
