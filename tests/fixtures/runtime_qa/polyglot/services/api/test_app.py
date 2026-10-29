from pathlib import Path
import unittest


class BuiltWebAppTests(unittest.TestCase):
    def test_build_emits_the_approved_runtime_files(self):
        root = Path(__file__).parent
        self.assertTrue((root / "dist" / "app.py").is_file())
        page = (root / "dist" / "index.html").read_text(encoding="utf-8")
        self.assertIn('for="message"', page)
        self.assertIn('data-testid="status"', page)
        self.assertIn("const savedMessage = await response.text();", page)

        server = (root / "dist" / "app.py").read_text(encoding="utf-8")
        self.assertIn('self.send_header("Content-Length", str(len(body)))', server)
        self.assertIn('body = b"Saved"', server)


if __name__ == "__main__":
    unittest.main()
