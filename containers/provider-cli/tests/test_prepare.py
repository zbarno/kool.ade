"""Production boundary: untrusted npm archives become executable image inputs.

These unit tests prove integrity and extraction validators reject tampering and
archive path tricks; they cannot prove the actual upstream binaries start in
the candidate image. That requires the image workflow's network download and
rootless Docker smoke on native amd64 and arm64 runners. Unit setup is local
and has no external dependencies.
"""

from __future__ import annotations

import io
import base64
import copy
import hashlib
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from prepare import _download_verified, _extract_binary, read_lock


class FakeResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def archive_with(name: str, kind: bytes = b"0", payload: bytes = b"binary") -> Path:
    temp = tempfile.NamedTemporaryFile(suffix=".tgz", delete=False)
    temp.close()
    with tarfile.open(temp.name, "w:gz") as archive:
        info = tarfile.TarInfo(name)
        info.size = len(payload) if kind == b"0" else 0
        info.type = kind
        archive.addfile(info, io.BytesIO(payload) if info.isfile() else None)
    return Path(temp.name)


class ProviderPackageExtractionTests(unittest.TestCase):
    def test_public_lock_pins_both_providers_for_supported_architectures(self) -> None:
        lock = read_lock()
        self.assertEqual(set(lock["packages"]), {
            "codex-linux-amd64", "codex-linux-arm64",
            "claude-linux-amd64", "claude-linux-arm64",
        })
        for package in lock["packages"].values():
            self.assertTrue(package["tarball"].startswith("https://registry.npmjs.org/"))
            self.assertTrue(package["integrity"].startswith("sha512-"))

    def test_lock_rejects_changed_package_source_or_extraction_path(self) -> None:
        valid = read_lock()
        for field, value in (("tarball", "https://example.invalid/package.tgz"),
                             ("archive_path", "package/../outside")):
            changed = copy.deepcopy(valid)
            changed["packages"]["codex-linux-amd64"][field] = value
            with tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "lock.json"
                path.write_text(json.dumps(changed), encoding="utf-8")
                with patch("prepare.LOCK", path):
                    with self.assertRaisesRegex(ValueError, "entry codex-linux-amd64"):
                        read_lock()

    def test_download_accepts_only_matching_sha512_integrity(self) -> None:
        payload = b"fixture package bytes"
        integrity = "sha512-" + base64.b64encode(hashlib.sha512(payload).digest()).decode()
        package = {
            "tarball": "https://registry.npmjs.org/fixture.tgz",
            "integrity": integrity,
        }
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "package.tgz"
            with patch("prepare.urllib.request.urlopen", return_value=FakeResponse(payload)):
                _download_verified(package, target)
            self.assertEqual(target.read_bytes(), payload)

    def test_download_removes_archive_with_wrong_integrity(self) -> None:
        package = {
            "tarball": "https://registry.npmjs.org/fixture.tgz",
            "integrity": "sha512-" + base64.b64encode(bytes(64)).decode(),
        }
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "package.tgz"
            with patch("prepare.urllib.request.urlopen", return_value=FakeResponse(b"tampered")):
                with self.assertRaisesRegex(ValueError, "does not match"):
                    _download_verified(package, target)
            self.assertFalse(target.exists())

    def test_extracts_only_the_expected_regular_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            archive = archive_with("package/vendor/x86_64-unknown-linux-musl/bin/codex")
            try:
                output = Path(temp) / "codex"
                _extract_binary(
                    archive,
                    "package/vendor/x86_64-unknown-linux-musl/bin/codex",
                    output,
                )
                self.assertEqual(output.read_bytes(), b"binary")
                self.assertEqual(output.stat().st_mode & 0o777, 0o555)
            finally:
                archive.unlink(missing_ok=True)

    def test_rejects_traversal_member_names(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            archive = archive_with("package/../../tmp/payload")
            try:
                with self.assertRaisesRegex(ValueError, "member path is invalid"):
                    _extract_binary(archive, "package/../../tmp/payload", Path(temp) / "out")
                self.assertFalse((Path(temp).parent / "payload").exists())
            finally:
                archive.unlink(missing_ok=True)

    def test_rejects_symbolic_links_even_when_the_path_matches(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            archive = archive_with("package/claude", kind=tarfile.SYMTYPE)
            try:
                with self.assertRaisesRegex(ValueError, "invalid file type or size"):
                    _extract_binary(archive, "package/claude", Path(temp) / "claude")
                self.assertFalse((Path(temp) / "claude").exists())
            finally:
                archive.unlink(missing_ok=True)

    def test_rejects_missing_expected_member(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            archive = archive_with("package/not-codex")
            try:
                with self.assertRaisesRegex(ValueError, "exactly one expected binary"):
                    _extract_binary(archive, "package/codex", Path(temp) / "codex")
            finally:
                archive.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
