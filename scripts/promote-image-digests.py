#!/usr/bin/env python3
"""Validate CI image evidence and prepare a reviewed digest-lock update."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCK_PATH = ROOT / "containers/images.lock.json"
BASE_LOCK_PATH = ROOT / "containers/base-images.lock.json"
REPOSITORIES = {
    "general": "ghcr.io/zbarno/koolade-executor",
    "rust": "ghcr.io/zbarno/koolade-executor",
    "node": "ghcr.io/zbarno/koolade-executor",
    "python": "ghcr.io/zbarno/koolade-executor",
    "go": "ghcr.io/zbarno/koolade-executor",
    "browser_qa": "ghcr.io/zbarno/koolade-runtime-qa-browser",
    "desktop_qa": "ghcr.io/zbarno/koolade-runtime-qa-desktop",
}
HEX_DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
REVISION = re.compile(r"^[0-9a-f]{40}$")


def fail(message: str) -> None:
    raise ValueError(message)


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read JSON file {path}: {error}")
    if not isinstance(value, dict):
        fail(f"expected a JSON object in {path}")
    return value


def validate_candidate(
    candidate: dict,
    base_lock: dict,
    source_revision: str | None,
    required_profiles: set[str],
    evidence_root: Path,
) -> list[dict]:
    if candidate.get("schema_version") != 1 or not isinstance(candidate.get("images"), list):
        fail("candidate manifest must use schema_version 1 and contain images")
    seen: set[tuple[str, str]] = set()
    validated = []
    for item in candidate["images"]:
        if not isinstance(item, dict):
            fail("each candidate image must be an object")
        profile, platform = item.get("profile"), item.get("platform")
        if profile not in REPOSITORIES or platform not in {"linux/amd64", "linux/arm64"}:
            fail(f"unsupported candidate profile/platform: {profile!r} {platform!r}")
        if profile in {"browser_qa", "desktop_qa"} and platform != "linux/amd64":
            fail(f"{profile} is currently supported only on linux/amd64")
        key = (profile, platform)
        if key in seen:
            fail(f"duplicate candidate image: {profile} {platform}")
        seen.add(key)
        digest = item.get("digest")
        repository = REPOSITORIES[profile]
        reference = item.get("reference")
        if not isinstance(digest, str) or not HEX_DIGEST.fullmatch(digest):
            fail(f"{profile} {platform} has no valid content digest")
        if reference != f"{repository}@{digest}":
            fail(f"{profile} {platform} must use its exact trusted repository and digest")

        revision = item.get("source_revision")
        if not isinstance(revision, str) or not REVISION.fullmatch(revision):
            fail(f"{profile} {platform} has no full source revision")
        if source_revision and revision != source_revision:
            fail(f"{profile} {platform} was built from a different source revision")
        base = next((entry for entry in base_lock.get("profiles", {}).get(profile, [])
                     if entry.get("platform") == platform), None)
        if not base or item.get("base_image") != base.get("reference"):
            fail(f"{profile} {platform} does not use its architecture-specific locked base")

        recipe = item.get("build_recipe_sha256")
        if not isinstance(recipe, str) or not HEX_DIGEST.fullmatch(recipe):
            fail(f"{profile} {platform} has no build recipe digest")
        architecture = platform.removeprefix("linux/")
        evidence_directory = evidence_root / f"executor-image-candidate-{profile}-{architecture}"
        try:
            expected_recipe = expected_recipe_hash(profile, evidence_directory)
        except OSError as error:
            fail(f"{profile} {platform} is missing recipe inputs: {error}")
        if recipe != expected_recipe:
            fail(f"{profile} {platform} was built from a different image recipe")
        for evidence in ("sbom_sha256", "provenance_sha256"):
            if not isinstance(item.get(evidence), str) or not HEX_DIGEST.fullmatch(item[evidence]):
                fail(f"{profile} {platform} has no valid {evidence} evidence digest")
        sbom_path = evidence_directory / "executor-sbom.spdx.json"
        provenance_path = evidence_directory / "executor-provenance.json"
        try:
            sbom_bytes = sbom_path.read_bytes()
            provenance_bytes = provenance_path.read_bytes()
        except OSError as error:
            fail(f"{profile} {platform} is missing its SBOM or provenance evidence: {error}")
        actual_sbom = "sha256:" + hashlib.sha256(sbom_bytes).hexdigest()
        actual_provenance = "sha256:" + hashlib.sha256(provenance_bytes).hexdigest()
        if item["sbom_sha256"] != actual_sbom:
            fail(f"{profile} {platform} SBOM digest does not match its evidence file")
        if item["provenance_sha256"] != actual_provenance:
            fail(f"{profile} {platform} provenance digest does not match its evidence file")
        try:
            sbom = json.loads(sbom_bytes)
        except json.JSONDecodeError as error:
            fail(f"{profile} {platform} SBOM evidence is invalid JSON: {error}")
        if not isinstance(sbom, dict) or not str(sbom.get("spdxVersion", "")).startswith("SPDX-2."):
            fail(f"{profile} {platform} SBOM evidence is not SPDX JSON")
        try:
            provenance = json.loads(provenance_bytes)
        except json.JSONDecodeError as error:
            fail(f"{profile} {platform} provenance evidence is invalid JSON: {error}")
        if not isinstance(provenance, dict):
            fail(f"{profile} {platform} provenance evidence must be an object")
        expected_subject = {
            "profile": profile,
            "platform": platform,
            "reference": reference,
            "digest": digest,
        }
        if (
            provenance.get("subject") != expected_subject
            or provenance.get("source_revision") != revision
            or provenance.get("sbom_sha256") != actual_sbom
            or provenance.get("base_image") != item["base_image"]
            or provenance.get("build_recipe_sha256") != recipe
        ):
            fail(f"{profile} {platform} provenance does not bind this candidate and evidence")
        versions = item.get("tool_versions")
        if not isinstance(versions, dict) or not versions or any(
            not isinstance(name, str) or not isinstance(version, str) or not version.strip()
            for name, version in versions.items()
        ):
            fail(f"{profile} {platform} has invalid tool version metadata")
        if profile == "rust" and (versions.get("rustc") != "1.98.1" or versions.get("cargo") != "1.98.1"):
            fail("Rust candidate tool versions must match the pinned 1.98.1 toolchain")
        if profile == "python" and versions.get("python") != "3.14.7":
            fail("Python candidate tool version must match 3.14.7")
        if profile == "go" and versions.get("go") != "1.26.8":
            fail("Go candidate tool version must match 1.26.8")
        required_versions = {
            "browser_qa": ("node", "npm", "python3", "playwright", "chromium", "driver", "base"),
            "desktop_qa": ("driver", "xvfb", "base"),
        }.get(profile, ())
        if any(key not in versions for key in required_versions):
            fail(f"{profile} candidate is missing required tool version metadata")
        validated.append(item)

    expected = {
        (profile, variant["platform"])
        for profile in required_profiles
        for variant in base_lock.get("profiles", {}).get(profile, [])
    }
    if seen != expected:
        missing, extra = sorted(expected - seen), sorted(seen - expected)
        fail(f"candidate set is incomplete (missing={missing}, extra={extra})")
    return validated


def recipe_sources(profile: str) -> list[tuple[Path, str]]:
    if profile in {"general", "rust", "node", "python", "go"}:
        paths = [ROOT / "containers/executor/Dockerfile", ROOT / "containers/executor/README.md"]
    elif profile == "browser_qa":
        directory = ROOT / "containers/runtime-qa/browser"
        paths = [
            directory / name
            for name in (
                "Dockerfile",
                "driver.mjs",
                "service.mjs",
                "relay.mjs",
                "package.json",
                "package-lock.json",
            )
        ] + sorted((directory / "vendor").glob("*.tgz")) + [
            ROOT / "scripts/build-qa-image-candidate.sh"
        ]
    elif profile == "desktop_qa":
        paths = [ROOT / "containers/runtime-qa/desktop/Dockerfile"]
    else:
        fail(f"no reviewed recipe source set for {profile}")
    return [(path, path.relative_to(ROOT).as_posix()) for path in paths]


def expected_recipe_hash(profile: str, evidence_directory: Path) -> str:
    sources = recipe_sources(profile)
    if profile == "desktop_qa":
        sources.append(
            (
                evidence_directory / "koolade-qa-desktop",
                "containers/runtime-qa/desktop/koolade-qa-desktop",
            )
        )
        sources.append(
            (ROOT / "scripts/build-qa-image-candidate.sh", "scripts/build-qa-image-candidate.sh")
        )
    payload = "".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {relative_path}\n"
        for path, relative_path in sources
    ).encode()
    return "sha256:" + hashlib.sha256(payload).hexdigest()


def find_locked_variant(lock: dict, profile: str, platform: str) -> dict | None:
    return next(
        (
            variant
            for variant in lock.get("profiles", {}).get(profile, [])
            if variant.get("platform") == platform
        ),
        None,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path, help="CI evidence manifest containing all image candidates")
    parser.add_argument("--evidence-root", type=Path, required=True, help="directory containing the downloaded per-image evidence artifacts")
    parser.add_argument("--source-revision", help="require every candidate to match this full git revision")
    parser.add_argument("--promote", action="store_true", help="write candidate digests into the checked-in lock for review")
    parser.add_argument("--check-lock", action="store_true", help="require candidates to exactly match the checked-in runtime lock")
    parser.add_argument("--require-profiles", nargs="*", choices=sorted(REPOSITORIES), default=[], help="require complete architecture coverage for these profiles")
    args = parser.parse_args()

    try:
        lock = read_json(LOCK_PATH)
        base_lock = read_json(BASE_LOCK_PATH)
        candidate = read_json(args.candidate)
        images = validate_candidate(
            candidate,
            base_lock,
            args.source_revision,
            set(args.require_profiles),
            args.evidence_root,
        )
        if args.promote and args.check_lock:
            fail("--promote and --check-lock cannot be used together")
        if args.promote and any(
            item["profile"] in {"browser_qa", "desktop_qa"} for item in images
        ) and not {"browser_qa", "desktop_qa"}.issubset(set(args.require_profiles)):
            fail("QA profiles must be promoted together with --require-profiles browser_qa desktop_qa")
        if args.check_lock:
            for item in images:
                profile = item["profile"]
                variant = find_locked_variant(lock, profile, item["platform"])
                if variant is None:
                    fail(f"runtime lock has no {profile} {item['platform']} image entry")
                if any(
                    variant.get(field) != item[source]
                    for field, source in (
                        ("reference", "reference"),
                        ("digest", "digest"),
                        ("tool_versions", "tool_versions"),
                        ("build_recipe_sha256", "build_recipe_sha256"),
                    )
                ):
                    fail(f"{profile} {variant.get('platform')} candidate differs from the reviewed image lock")
        if args.promote:
            indexed = {(item["profile"], item["platform"]): item for item in images}
            for profile, platform in indexed:
                variant = find_locked_variant(lock, profile, platform)
                if variant is None:
                    if profile not in {"browser_qa", "desktop_qa"}:
                        fail(f"runtime lock has no {profile} {platform} image entry")
                    variant = {"platform": platform, "runtime_enabled": False}
                    lock.setdefault("profiles", {}).setdefault(profile, []).append(variant)
                item = indexed[(profile, platform)]
                variant["reference"] = item["reference"]
                variant["digest"] = item["digest"]
                variant["tool_versions"] = item["tool_versions"]
                variant["build_recipe_sha256"] = item["build_recipe_sha256"]
                if profile in {"browser_qa", "desktop_qa"}:
                    # A verified image digest is not a runtime-QA release
                    # approval. Enabling remains a separate #171 release step.
                    variant["runtime_enabled"] = False
            LOCK_PATH.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8")
            print(f"Updated {LOCK_PATH}; review the diff and submit it through a pull request.")
        else:
            print(f"Validated {len(images)} image candidates and their base, recipe, SBOM, and provenance evidence.")
        return 0
    except (KeyError, TypeError, ValueError) as error:
        print(f"image digest promotion rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
