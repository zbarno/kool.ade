from pathlib import Path
import unittest


class BuiltWebAppTests(unittest.TestCase):
    def test_build_emits_the_approved_runtime_files(self):
        root = Path(__file__).parent
        self.assertTrue((root / "dist" / "app.py").is_file())
        page = (root / "dist" / "index.html").read_text(encoding="utf-8")
        self.assertIn('for="message"', page)
        self.assertIn('data-testid="status"', page)


if __name__ == "__main__":
    unittest.main()
