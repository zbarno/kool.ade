#!/usr/bin/env python3
"""Fetch integrity-pinned provider binaries into an ephemeral image build context."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import shutil
import tarfile
import tempfile
import urllib.error
import urllib.request
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent
LOCK = ROOT / "packages.lock.json"
MAX_ARCHIVE_BYTES = 800 * 1024 * 1024
MAX_BINARY_BYTES = 600 * 1024 * 1024
ARCHIVE_CHUNK_BYTES = 1024 * 1024
EXPECTED_BINARIES = ("codex", "claude")
EXPECTED_PACKAGES = {
    "codex-linux-amd64": (
        "@openai/codex",
        "package/vendor/x86_64-unknown-linux-musl/bin/codex",
        "codex",
    ),
    "codex-linux-arm64": (
        "@openai/codex",
        "package/vendor/aarch64-unknown-linux-musl/bin/codex",
        "codex",
    ),
    "claude-linux-amd64": (
        "@anthropic-ai/claude-code-linux-x64",
        "package/claude",
        "claude",
    ),
    "claude-linux-arm64": (
        "@anthropic-ai/claude-code-linux-arm64",
        "package/claude",
        "claude",
    ),
}


def read_lock() -> dict:
    value = json.loads(LOCK.read_text(encoding="utf-8"))
    if value.get("schema_version") != 1 or not isinstance(value.get("packages"), dict):
        raise ValueError("invalid provider package lock")
    packages = value["packages"]
    if set(packages) != set(EXPECTED_PACKAGES):
        raise ValueError("provider package lock has incomplete or unsupported entries")
    for key, (expected_name, expected_path, expected_binary) in EXPECTED_PACKAGES.items():
        package = packages[key]
        if not isinstance(package, dict):
            raise ValueError(f"provider package lock entry {key} is malformed")
        name = package.get("name")
        version = package.get("version")
        tarball = package.get("tarball")
        if (
            name != expected_name
            or not isinstance(version, str)
            or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version)
        ):
            raise ValueError(f"provider package lock entry {key} is malformed")
        expected_tarball = f"https://registry.npmjs.org/{name}/-/{name.rsplit('/', 1)[-1]}-{version}.tgz"
        if (
            tarball != expected_tarball
            or package.get("archive_path") != expected_path
            or package.get("binary") != expected_binary
            or not re.fullmatch(r"sha512-[A-Za-z0-9+/]{86}==", package.get("integrity", ""))
        ):
            raise ValueError(f"provider package lock entry {key} is malformed")
    return value


def _download_verified(package: dict, destination: Path) -> None:
    if not package["tarball"].startswith("https://registry.npmjs.org/"):
        raise ValueError("provider package source is outside the public npm registry")
    expected = package["integrity"]
    if not re.fullmatch(r"sha512-[A-Za-z0-9+/]{86}==", expected):
        raise ValueError("provider package integrity is invalid")
    digest = hashlib.sha512()
    total = 0
    request = urllib.request.Request(
        package["tarball"], headers={"User-Agent": "koolade-provider-image-builder/1"}
    )
    with urllib.request.urlopen(request, timeout=60) as response, destination.open("wb") as output:
        while chunk := response.read(ARCHIVE_CHUNK_BYTES):
            total += len(chunk)
            if total > MAX_ARCHIVE_BYTES:
                raise ValueError("provider package archive exceeds the configured bound")
            digest.update(chunk)
            output.write(chunk)
    actual = "sha512-" + base64.b64encode(digest.digest()).decode("ascii")
    if actual != expected:
        destination.unlink(missing_ok=True)
        raise ValueError("provider package integrity does not match the reviewed lock")


def _extract_binary(archive: Path, archive_path: str, destination: Path) -> None:
    member_path = PurePosixPath(archive_path)
    if member_path.is_absolute() or ".." in member_path.parts:
        raise ValueError("provider package member path is invalid")
    with tarfile.open(archive, mode="r:gz") as package:
        matches = [member for member in package if member.name == archive_path]
        if len(matches) != 1:
            raise ValueError("provider package does not contain exactly one expected binary")
        member = matches[0]
        if not member.isfile() or member.size <= 0 or member.size > MAX_BINARY_BYTES:
            raise ValueError("provider package binary has an invalid file type or size")
        source = package.extractfile(member)
        if source is None:
            raise ValueError("provider package binary could not be read")
        with destination.open("xb") as output:
            shutil.copyfileobj(source, output, length=ARCHIVE_CHUNK_BYTES)
        if destination.stat().st_size != member.size:
            destination.unlink(missing_ok=True)
            raise ValueError("provider package binary size did not match its archive metadata")
        destination.chmod(0o555)


def prepare(architecture: str, output: Path) -> None:
    lock = read_lock()
    suffix = "amd64" if architecture == "x64" else "arm64"
    package_keys = (f"codex-linux-{suffix}", f"claude-linux-{suffix}")
    output.mkdir(parents=True, exist_ok=False)
    try:
        for key in package_keys:
            package = lock["packages"][key]
            with tempfile.TemporaryDirectory(prefix="koolade-provider-package-") as temp:
                archive = Path(temp) / "package.tgz"
                _download_verified(package, archive)
                _extract_binary(archive, package["archive_path"], output / package["binary"])
        if tuple(sorted(path.name for path in output.iterdir())) != tuple(sorted(EXPECTED_BINARIES)):
            raise ValueError("provider binary staging directory is incomplete")
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--architecture", choices=("x64", "arm64"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        prepare(args.architecture, args.output)
    except (KeyError, OSError, tarfile.TarError, ValueError, urllib.error.URLError) as error:
        parser.error(f"provider image preparation failed: {error}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
