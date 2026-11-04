import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-doc-navigation.py"
SPEC = importlib.util.spec_from_file_location("doc_navigation", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
NAVIGATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NAVIGATION)


class NavigationTests(unittest.TestCase):
    def test_navigation_section_update_preserves_other_agent_content(self):
        original = "# Local policy\n\nKeep this policy.\n\n# Navigation\n\n- stale\n"
        updated = NAVIGATION.replace_navigation(
            original,
            "# Navigation\n\n- [guide.md](./guide.md) - Read the guide.\n",
        )
        self.assertIn("# Local policy\n\nKeep this policy.", updated)
        self.assertIn("[guide.md](./guide.md)", updated)
        self.assertNotIn("- stale", updated)

    def test_checker_detects_broken_and_incomplete_immediate_links(self):
        with tempfile.TemporaryDirectory(dir=NAVIGATION.ROOT) as temporary:
            directory = Path(temporary)
            (directory / "guide.md").write_text("# Guide\n", encoding="utf-8")
            (directory / "AGENTS.md").write_text(
                "# Navigation\n\n- [lost.md](./lost.md) - Missing page.\n",
                encoding="utf-8",
            )
            errors = NAVIGATION.check(directory)
        self.assertTrue(any("broken navigation link" in error for error in errors))
        self.assertTrue(any("stale or incomplete" in error for error in errors))

    def test_repository_navigation_indexes_cover_immediate_documents(self):
        errors = [
            error
            for directory in NAVIGATION.document_directories()
            for error in NAVIGATION.check(directory)
        ]
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
