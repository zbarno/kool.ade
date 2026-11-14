import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BUILDER = ROOT / "containers/computer-use/make-sbom.py"


class GuestImageSbomTests(unittest.TestCase):
    def build_sbom(self, architecture=None):
        with tempfile.TemporaryDirectory() as temporary:
            package_list = Path(temporary) / "packages.tsv"
            output = Path(temporary) / "sbom.json"
            package_list.write_text("chromium\t1:2.0\n", encoding="utf-8")
            command = ["python3", str(BUILDER), str(package_list), str(output)]
            if architecture is not None:
                command.append(architecture)
            subprocess.run(command, check=True, capture_output=True, text=True)
            return json.loads(output.read_text(encoding="utf-8"))

    def test_default_sbom_architecture_preserves_x86_64_metadata(self):
        bom = self.build_sbom()
        self.assertEqual(
            bom["components"][0]["purl"],
            "pkg:deb/debian/chromium@1%3A2.0?arch=amd64",
        )

    def test_aarch64_guest_sbom_uses_debian_arm64_metadata(self):
        bom = self.build_sbom("arm64")
        self.assertEqual(
            bom["components"][0]["purl"],
            "pkg:deb/debian/chromium@1%3A2.0?arch=arm64",
        )

    def test_sbom_rejects_unrecognized_architecture(self):
        with tempfile.TemporaryDirectory() as temporary:
            package_list = Path(temporary) / "packages.tsv"
            package_list.write_text("chromium\t1:2.0\n", encoding="utf-8")
            result = subprocess.run(
                ["python3", str(BUILDER), str(package_list), str(Path(temporary) / "out"), "arm"],
                capture_output=True,
                text=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported Debian package architecture", result.stderr)


if __name__ == "__main__":
    unittest.main()
