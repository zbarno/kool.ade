"""Deterministic, network-free image-promotion policy and evidence regression tests."""

import copy
import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import image_lock_policy as policy  # noqa: E402

spec = importlib.util.spec_from_file_location("promote_image_digests", SCRIPTS / "promote-image-digests.py")
promote = importlib.util.module_from_spec(spec)
spec.loader.exec_module(promote)

REV = "a" * 40
OTHER_REV = "b" * 40
BASE = "mcr.microsoft.com/playwright@sha256:" + "c" * 64
RECIPE = "sha256:" + "d" * 64
SBOM = {"spdxVersion": "SPDX-2.3"}
BROWSER_TOOLS = {
    "node": "24.11.1", "npm": "11.6.2", "python3": "Python 3.12.3",
    "playwright": "1.57.0", "chromium": "143.0.7499.4",
    "driver": "browser-1.1", "base": BASE,
}
DESKTOP_TOOLS = {
    "driver": "linux-x11-driver/v1", "xvfb": "version", "chromium": "143.0.7499.4",
    "dbus": "version", "base": BASE,
}


def lock():
    profiles = {}
    for name in (*policy.EXECUTOR_PROFILES, *policy.QA_PROFILES):
        digest = "sha256:" + hashlib.sha256(name.encode()).hexdigest()
        profile = [{
            "platform": "linux/amd64", "digest": digest,
            "reference": policy.REPOSITORIES[name] + "@" + digest,
            "build_recipe_sha256": RECIPE,
            "tool_versions": (
                BROWSER_TOOLS if name == "browser_qa" else
                DESKTOP_TOOLS if name == "desktop_qa" else {"base": "test"}
            ),
        }]
        if name in policy.QA_PROFILES:
            profile[0]["runtime_enabled"] = False
        profiles[name] = profile
    return {"schema_version": 1, "image_set_version": "1", "profiles": profiles}


def base_lock():
    return {
        "schema_version": 1,
        "profiles": {
            name: [{"platform": "linux/amd64", "reference": BASE}]
            for name in (*policy.EXECUTOR_PROFILES, *policy.QA_PROFILES)
        },
    }


def candidate(manifest, profile):
    variant = manifest["profiles"][profile][0]
    return {
        **{field: copy.deepcopy(variant[field]) for field in policy.COMPARE_FIELDS},
        "profile": profile, "platform": "linux/amd64",
        "base_image": BASE, "source_revision": REV,
        "sbom_sha256": "sha256:" + "1" * 64,
        "provenance_sha256": "sha256:" + "2" * 64,
    }


class ClassificationTests(unittest.TestCase):
    def test_unchanged_lock_and_source_only_change(self):
        before = lock()
        after = copy.deepcopy(before)
        after["profiles"] = dict(reversed(list(after["profiles"].items())))
        self.assertEqual(policy.image_family_changes(before, after), {
            "executor_lock_changed": False, "qa_lock_changed": False
        })

    def test_qa_only_and_executor_only_promotion(self):
        before = lock()
        qa = copy.deepcopy(before)
        qa["profiles"]["desktop_qa"][0]["digest"] = "sha256:" + "f" * 64
        self.assertEqual(policy.image_family_changes(before, qa), {
            "executor_lock_changed": False, "qa_lock_changed": True
        })
        executor = copy.deepcopy(before)
        executor["profiles"]["rust"][0]["digest"] = "sha256:" + "e" * 64
        self.assertEqual(policy.image_family_changes(before, executor), {
            "executor_lock_changed": True, "qa_lock_changed": False
        })

    def test_mixed_and_image_set_version_changes(self):
        before = lock()
        after = copy.deepcopy(before)
        after["profiles"]["rust"][0]["digest"] = "sha256:" + "e" * 64
        after["profiles"]["browser_qa"][0]["digest"] = "sha256:" + "f" * 64
        self.assertTrue(all(policy.image_family_changes(before, after).values()))
        after = copy.deepcopy(before)
        after["image_set_version"] = "2"
        self.assertTrue(all(policy.image_family_changes(before, after).values()))

    def test_changed_paths_select_only_affected_image_families(self):
        cases = (
            (["src/execution/qa.rs"], {"executor": False, "qa": True}),
            (["containers/runtime-qa/desktop/Dockerfile"], {"executor": False, "qa": True}),
            (["containers/executor/Dockerfile"], {"executor": True, "qa": False}),
            (["Cargo.lock"], {"executor": False, "qa": True}),
            (["src/lib.rs", "containers/executor/Dockerfile"], {"executor": True, "qa": True}),
            (["containers/base-images.lock.json"], {"executor": True, "qa": True}),
            (["containers/images.lock.json"], {"executor": True, "qa": True}),
            (["docs/architecture.md"], {"executor": False, "qa": False}),
        )
        for paths, expected in cases:
            with self.subTest(paths=paths):
                self.assertEqual(policy.affected_builds(paths), expected)

    def test_malformed_manifest_fails_closed(self):
        before = lock()
        for bad in ({"schema_version": 1, "profiles": {}}, {"schema_version": 2, "profiles": {}}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                policy.image_family_changes(before, bad)

    def test_cli_classifies_json_without_code_evaluation(self):
        with tempfile.TemporaryDirectory() as root:
            before_path = Path(root) / "before.json"
            after_path = Path(root) / "after.json"
            before_path.write_text(json.dumps(lock()))
            after = lock()
            after["profiles"]["desktop_qa"][0]["digest"] = "sha256:" + "f" * 64
            after_path.write_text(json.dumps(after))
            parsed = policy.image_family_changes(policy.read_json(before_path), policy.read_json(after_path))
            self.assertFalse(parsed["executor_lock_changed"])
            self.assertTrue(parsed["qa_lock_changed"])


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.manifest = lock()
        self.bases = base_lock()

    def test_verified_browser_and_desktop_candidate_identities(self):
        for profile in policy.QA_PROFILES:
            with self.subTest(profile=profile):
                policy.verify_candidate(self.manifest, self.bases, candidate(self.manifest, profile),
                                        profile, "linux/amd64", REV)

    def test_mismatched_fields_fail_with_public_hash_diagnostics(self):
        c = candidate(self.manifest, "desktop_qa")
        c["digest"] = "sha256:" + "b" * 64
        c["reference"] = policy.REPOSITORIES["desktop_qa"] + "@" + c["digest"]
        c["build_recipe_sha256"] = "sha256:" + "9" * 64
        c["tool_versions"] = {"sensitive-looking-name": "dont-print-me"}
        with self.assertRaisesRegex(ValueError, "digest.*build_recipe_sha256.*tool_versions") as raised:
            policy.verify_candidate(self.manifest, self.bases, c, "desktop_qa", "linux/amd64", REV)
        self.assertNotIn("dont-print-me", str(raised.exception))
        self.assertIn("sha256:", str(raised.exception))

    def test_wrong_source_base_missing_evidence_or_duplicate_variant_fails(self):
        profile = "browser_qa"
        good = candidate(self.manifest, profile)
        for key, value in (("source_revision", OTHER_REV), ("base_image", "fake"),
                           ("sbom_sha256", ""), ("provenance_sha256", None)):
            c = dict(good, **{key: value})
            with self.subTest(key=key), self.assertRaises(ValueError):
                policy.verify_candidate(self.manifest, self.bases, c, profile, "linux/amd64", REV)
        self.manifest["profiles"][profile].append(copy.deepcopy(self.manifest["profiles"][profile][0]))
        with self.assertRaisesRegex(ValueError, "exactly one"):
            policy.verify_candidate(self.manifest, self.bases, good, profile, "linux/amd64", REV)

    def test_runtime_qa_cannot_be_activated_by_promotion(self):
        self.manifest["profiles"]["desktop_qa"][0]["runtime_enabled"] = True
        with self.assertRaisesRegex(ValueError, "#171"):
            policy.verify_candidate(self.manifest, self.bases, candidate(self.manifest, "desktop_qa"),
                                    "desktop_qa", "linux/amd64", REV)

    def test_invalid_trusted_repository_or_digest_rejected(self):
        c = candidate(self.manifest, "browser_qa")
        c["reference"] = "docker.io/malicious@" + c["digest"]
        with self.assertRaisesRegex(ValueError, "trusted"):
            policy.verify_candidate(self.manifest, self.bases, c, "browser_qa", "linux/amd64", REV)


class PublicationOrderingTests(unittest.TestCase):
    """Keep credential and push boundaries visible in source-level CI contracts."""

    def test_qa_publisher_preflights_before_first_push(self):
        source = (SCRIPTS / "publish-qa-image-candidate.sh").read_text()
        self.assertLess(source.index("python3 scripts/image_lock_policy.py verify"),
                        source.index("push=true"))
        self.assertLess(source.index('push=false,rewrite-timestamp=true'),
                        source.index('push=true,rewrite-timestamp=true'))
        self.assertLess(source.index('preflight_digest" != "$expected_digest"'),
                        source.index('push=true,rewrite-timestamp=true'))

    def test_workflow_preflights_both_families_before_authentication(self):
        workflow = (SCRIPTS.parent / ".github/workflows/executor-images.yml").read_text()
        qa = workflow.split("  publish-reviewed-qa-main:", 1)[1].split("  smoke-reviewed-qa-main:", 1)[0]
        executor = workflow.split("  publish-reviewed-main:", 1)[1]
        self.assertLess(qa.index("Verify reviewed candidate before registry authentication"),
                        qa.index("Authenticate for the fixed image publication step"))
        self.assertLess(executor.index("Verify exported executor candidate before registry authentication"),
                        executor.index("Authenticate to GitHub Container Registry"))
        self.assertLess(executor.index("push=false,rewrite-timestamp=true"),
                        executor.index("push=true,rewrite-timestamp=true"))


class EvidenceTests(unittest.TestCase):
    """Exercise the existing full SBOM/provenance validator with synthetic artifacts."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bases = base_lock()
        self.manifest = lock()
        self.images = []
        for profile in policy.QA_PROFILES:
            directory = self.root / f"executor-image-candidate-{profile}-amd64"
            directory.mkdir()
            record = candidate(self.manifest, profile)
            sbom_bytes = json.dumps(SBOM).encode()
            (directory / "executor-sbom.spdx.json").write_bytes(sbom_bytes)
            record["sbom_sha256"] = "sha256:" + hashlib.sha256(sbom_bytes).hexdigest()
            provenance = {
                "subject": {
                    "profile": profile, "platform": "linux/amd64",
                    "reference": record["reference"], "digest": record["digest"],
                },
                "source_revision": REV, "sbom_sha256": record["sbom_sha256"],
                "base_image": BASE, "build_recipe_sha256": RECIPE,
            }
            provenance_bytes = json.dumps(provenance).encode()
            (directory / "executor-provenance.json").write_bytes(provenance_bytes)
            record["provenance_sha256"] = "sha256:" + hashlib.sha256(provenance_bytes).hexdigest()
            self.images.append(record)

    def validate(self, items=None, revision=REV):
        with mock.patch.object(promote, "expected_recipe_hash", return_value=RECIPE):
            return promote.validate_candidate(
                {"schema_version": 1, "images": self.images if items is None else items},
                self.bases, revision, set(policy.QA_PROFILES), self.root,
            )

    def test_complete_verified_browser_and_desktop_evidence_pass(self):
        self.assertEqual(len(self.validate()), 2)

    def test_wrong_source_or_duplicate_fails(self):
        with self.assertRaisesRegex(ValueError, "different source revision"):
            self.validate(revision=OTHER_REV)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.validate(items=[*self.images, self.images[0]])

    def test_tampered_or_missing_sbom_and_provenance_fail_even_without_lock_change(self):
        browser_dir = self.root / "executor-image-candidate-browser_qa-amd64"
        for name in ("executor-sbom.spdx.json", "executor-provenance.json"):
            artifact = browser_dir / name
            original = artifact.read_bytes()
            try:
                artifact.write_bytes(original + b"tamper")
                with self.subTest(name=name), self.assertRaises(ValueError):
                    self.validate()
                artifact.unlink()
                with self.subTest(missing=name), self.assertRaises(ValueError):
                    self.validate()
            finally:
                artifact.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
