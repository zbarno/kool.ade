import importlib.util
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-computer-use-doc-status.py"
SPEC = importlib.util.spec_from_file_location("computer_use_doc_status", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
STATUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STATUS)


def docs():
    return {
        "docs/computer-use-architecture.md": "promoted guest image protocol v1 InternetResearch",
        "docs/computer-use-architecture-v1.md": "promoted protocol v1",
        "docs/computer-use-readiness-v1.md": "verified image cache",
        "docs/computer-use-threat-model-v1.md": "guest threat model",
        "docs/runtime-qa-operations.md": "Production Runtime QA remains **Unavailable**",
        "README.md": "Runtime QA profiles remain disabled; signed guest image is published",
        "site/architecture.html": "promoted signed x86_64 guest image; protocol v1",
    }


def flags():
    return (
        {"status": "promoted"},
        {"images": [{"companion_protocol_version": 1}]},
        {
            "profiles": {
                "browser_qa": [{"runtime_enabled": False}],
                "desktop_qa": [{"runtime_enabled": False}],
            }
        },
    )


class ComputerUseDocStatusTests(unittest.TestCase):
    def test_current_docs_match_guest_catalog_and_runtime_qa_flags(self):
        errors = STATUS.validate_status(*flags(), docs())
        self.assertEqual(errors, [])

    def test_stale_unpromoted_claim_is_rejected_after_promotion(self):
        image_lock, catalog, runtime_lock = flags()
        stale = docs()
        stale["docs/computer-use-readiness-v1.md"] = (
            "this build's lock is explicitly `not_promoted` and empty"
        )
        errors = STATUS.validate_status(image_lock, catalog, runtime_lock, stale)
        self.assertTrue(any("still claims the bundled image is not promoted" in item for item in errors))

    def test_enabling_a_qa_profile_requires_current_qa_release_copy(self):
        image_lock, catalog, runtime_lock = flags()
        runtime_lock["profiles"]["browser_qa"][0]["runtime_enabled"] = True
        stale = docs()
        stale["docs/runtime-qa-operations.md"] = "QA is unavailable"
        errors = STATUS.validate_status(image_lock, catalog, runtime_lock, stale)
        self.assertTrue(any("still says unavailable" in item for item in errors))


if __name__ == "__main__":
    unittest.main()
