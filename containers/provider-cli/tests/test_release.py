import hashlib
import json
import runpy
import tempfile
import unittest
from pathlib import Path


PROVIDER = Path(__file__).resolve().parents[1]
REPO = PROVIDER.parents[1]
release = runpy.run_path(str(PROVIDER / "verify-release.py"))


class ReleaseEvidenceTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.directory = Path(tmp.name)
        self.blobs = self.directory / "oci/blobs/sha256"
        self.blobs.mkdir(parents=True)
        self.revision = "a" * 40
        self.run = {
            "id": 17, "run_attempt": 2, "head_sha": self.revision, "head_branch": "main",
            "head_repository": {"full_name": "zbarno/kool.ade"},
            "path": ".github/workflows/provider-cli-image.yml",
            "event": "workflow_dispatch", "status": "completed", "conclusion": "success",
        }
        config = self.add_blob({"os": "linux", "architecture": "amd64", "config": {"User": "10001:10001"}})
        self.layer = self.add_blob({"fixture": True})
        image = {"config": config, "layers": [self.layer]}
        manifest = self.add_blob(image)
        self.write("oci/index.json", {"manifests": [manifest]})
        self.write("manifest.json", image)
        self.write("sbom.spdx.json", {"spdxVersion": "SPDX-2.3"})
        bases = release["read"](REPO / "containers/base-images.lock.json")["profiles"]["node"]
        record = {
            "schemaVersion": 1, "sourceRevision": self.revision, "runId": "17", "runAttempt": "2",
            "platform": "linux/amd64", "manifestDigest": manifest["digest"],
            "baseImage": next(item["reference"] for item in bases if item["platform"] == "linux/amd64"),
            "dockerfileDigest": release["digest"](PROVIDER / "Dockerfile"),
            "packageLockDigest": release["digest"](PROVIDER / "packages.lock.json"),
            "sbomDigest": release["digest"](self.directory / "sbom.spdx.json"),
            "checks": ["native-cli-startup", "synthetic-hook-container"],
        }
        self.write("candidate.json", record)
        self.approved = {"manifestDigest": manifest["digest"],
                         "candidateDigest": release["digest"](self.directory / "candidate.json")}
        self.lock = {"schemaVersion": 1, "sourceRevision": self.revision, "runId": 17, "runAttempt": 2,
                     "platforms": {"linux/amd64": self.approved, "linux/arm64": {}}}

    def write(self, name, value):
        (self.directory / name).write_text(json.dumps(value))

    def add_blob(self, value):
        data = json.dumps(value).encode()
        digest = hashlib.sha256(data).hexdigest()
        (self.blobs / digest).write_bytes(data)
        return {"digest": "sha256:" + digest, "size": len(data)}

    def verify(self):
        return release["verify"](self.lock, self.run, self.directory, "amd64", REPO)

    def test_verified_platform(self):
        self.assertEqual(self.verify(), self.approved["manifestDigest"])

    def test_untrusted_or_stale_run(self):
        for key, value in [("head_branch", "topic"), ("event", "pull_request"), ("run_attempt", 1),
                           ("head_repository", {"full_name": "example/fork"}), ("conclusion", "failure")]:
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "successful main workflow"):
                release["verify_source"](self.lock, {**self.run, key: value})

    def test_missing_platform(self):
        del self.lock["platforms"]["linux/arm64"]
        with self.assertRaisesRegex(ValueError, "both Linux"):
            self.verify()

    def test_changed_layer(self):
        (self.blobs / self.layer["digest"][7:]).write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "size or digest"):
            self.verify()

    def test_changed_sbom(self):
        self.write("sbom.spdx.json", {})
        with self.assertRaisesRegex(ValueError, "SBOM digest"):
            self.verify()

    def test_linked_record(self):
        record = self.directory / "candidate.json"
        record.rename(self.directory / "other")
        record.symlink_to("other")
        with self.assertRaisesRegex(ValueError, "linked artifact"):
            self.verify()
