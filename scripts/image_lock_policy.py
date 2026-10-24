#!/usr/bin/env python3
"""Classify reviewed image lock changes and fail closed before publishing candidates.

This is deliberately dependency-free so both GitHub CI and local review can run it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

EXECUTOR_PROFILES = ("general", "rust", "node", "python", "go")
QA_PROFILES = ("browser_qa", "desktop_qa")
REPOSITORIES = {
    **{name: "ghcr.io/zbarno/koolade-executor" for name in EXECUTOR_PROFILES},
    "browser_qa": "ghcr.io/zbarno/koolade-runtime-qa-browser",
    "desktop_qa": "ghcr.io/zbarno/koolade-runtime-qa-desktop",
}
SHA256 = re.compile(r"^sha256:[a-f0-9]{64}$")
REVISION = re.compile(r"^[a-f0-9]{40}$")
COMPARE_FIELDS = ("reference", "digest", "build_recipe_sha256", "tool_versions")


def read_json(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"{path}: expected JSON object")
    return data


def image_family_changes(old: dict, new: dict) -> dict[str, bool]:
    """Compare semantic JSON, not line-oriented formatting or unrelated profiles."""
    for record in (old, new):
        if record.get("schema_version") != 1 or not isinstance(record.get("profiles"), dict):
            raise ValueError("image manifest requires schema_version 1 and profiles")
    def view(record: dict, names: tuple[str, ...]) -> dict:
        profiles = record["profiles"]
        if any(not isinstance(profiles.get(name), list) for name in names):
            raise ValueError("missing image profile entries")
        return {
            "image_set_version": record.get("image_set_version"),
            "profiles": {name: profiles[name] for name in names},
        }
    return {
        "executor_lock_changed": view(old, EXECUTOR_PROFILES) != view(new, EXECUTOR_PROFILES),
        "qa_lock_changed": view(old, QA_PROFILES) != view(new, QA_PROFILES),
    }


def affected_builds(paths: list[str]) -> dict[str, bool]:
    """Conservative recipe/dependency selection; ordinary Rust never rebuilds executors."""
    both = {
        "containers/base-images.lock.json",
        "containers/images.lock.json",
        ".github/workflows/executor-images.yml",
    }
    qa_files = {"Cargo.toml", "Cargo.lock", "build.rs", ".config/nextest.toml"}
    executor = any(
        path in both
        or path.startswith(("containers/executor/", "containers/keys/", "scripts/"))
        for path in paths
    )
    qa = any(
        path in both or path in qa_files
        or path.startswith((
            "containers/runtime-qa/", "containers/keys/", "scripts/",
            "src/", "tests/", "assets/",
        ))
        for path in paths
    )
    return {"executor": executor, "qa": qa}


def _fingerprint(value: object) -> str:
    if value is None:
        return "<missing>"
    if isinstance(value, str) and SHA256.fullmatch(value):
        return value
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def identity_mismatches(approved: dict, candidate: dict) -> list[str]:
    """Only expose public digests or stable hashes, never arbitrary manifest strings."""
    return [
        f"{field}: approved={_fingerprint(approved.get(field))} candidate={_fingerprint(candidate.get(field))}"
        for field in COMPARE_FIELDS
        if approved.get(field) != candidate.get(field)
    ]


def find_variant(lock: dict, profile: str, platform: str) -> dict:
    matches = [
        entry for entry in lock.get("profiles", {}).get(profile, [])
        if isinstance(entry, dict) and entry.get("platform") == platform
    ]
    if len(matches) != 1:
        raise ValueError(f"{profile}/{platform}: expected exactly one approved variant, found {len(matches)}")
    return matches[0]


def verify_candidate(
    lock: dict, base_lock: dict, candidate: dict,
    profile: str, platform: str, revision: str,
) -> None:
    if profile not in REPOSITORIES or platform not in ("linux/amd64", "linux/arm64"):
        raise ValueError("unsupported image profile/platform")
    if profile in QA_PROFILES and platform != "linux/amd64":
        raise ValueError("QA images are currently supported only on linux/amd64")
    if not REVISION.fullmatch(revision) or candidate.get("source_revision") != revision:
        raise ValueError(f"{profile}/{platform}: candidate source revision does not match this build")
    if candidate.get("profile") != profile or candidate.get("platform") != platform:
        raise ValueError(f"{profile}/{platform}: candidate record identity does not match")
    approved = find_variant(lock, profile, platform)
    locked_base = find_variant(base_lock, profile, platform)
    if candidate.get("base_image") != locked_base.get("reference"):
        raise ValueError(f"{profile}/{platform}: candidate base does not match approved base")
    for field in ("digest", "build_recipe_sha256", "sbom_sha256", "provenance_sha256"):
        if not isinstance(candidate.get(field), str) or not SHA256.fullmatch(candidate[field]):
            raise ValueError(f"{profile}/{platform}: invalid candidate {field}")
    expected_ref = REPOSITORIES[profile] + "@" + candidate["digest"]
    if candidate.get("reference") != expected_ref:
        raise ValueError(f"{profile}/{platform}: candidate repository/reference does not match trusted digest")
    if approved.get("reference") != REPOSITORIES[profile] + "@" + approved.get("digest", ""):
        raise ValueError(f"{profile}/{platform}: approved reference is not the trusted repository/digest")
    if not isinstance(candidate.get("tool_versions"), dict) or not candidate["tool_versions"]:
        raise ValueError(f"{profile}/{platform}: missing candidate tool versions")
    if profile in QA_PROFILES and approved.get("runtime_enabled") is not False:
        raise ValueError(f"{profile}/{platform}: QA runtime must remain disabled pending #171 approval")
    mismatches = identity_mismatches(approved, candidate)
    if mismatches:
        raise ValueError(f"{profile}/{platform}: reviewed image identity mismatch: " + "; ".join(mismatches))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    select = sub.add_parser("select", help="select build families from git diff --name-only -z")
    select.add_argument("paths", type=Path)
    changed = sub.add_parser("classify", help="print changed family flags as JSON")
    changed.add_argument("old", type=Path)
    changed.add_argument("new", type=Path)
    verify = sub.add_parser("verify", help="fail closed before authentication or image publication")
    verify.add_argument("--lock", required=True, type=Path)
    verify.add_argument("--base-lock", required=True, type=Path)
    verify.add_argument("--candidate", required=True, type=Path)
    verify.add_argument("--profile", required=True, choices=sorted(REPOSITORIES))
    verify.add_argument("--platform", required=True)
    verify.add_argument("--source-revision", required=True)
    args = parser.parse_args()
    try:
        if args.command == "select":
            raw = args.paths.read_bytes()
            if raw and not raw.endswith(b"\0"):
                raise ValueError("changed-path list must be NUL delimited")
            paths = [value.decode("utf-8") for value in raw.split(b"\0") if value]
            print(json.dumps(affected_builds(paths), sort_keys=True))
        elif args.command == "classify":
            print(json.dumps(image_family_changes(read_json(args.old), read_json(args.new)), sort_keys=True))
        else:
            verify_candidate(
                read_json(args.lock), read_json(args.base_lock),
                read_json(args.candidate), args.profile, args.platform, args.source_revision,
            )
            print(f"Verified reviewed image identity: {args.profile}/{args.platform}")
        return 0
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        print(f"image promotion policy rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
