from pathlib import Path
import unittest


class FalseHealthyWebTests(unittest.TestCase):
    def test_build_keeps_health_route_and_broken_user_feedback(self):
        root = Path(__file__).parent
        page = (root / "dist" / "index.html").read_text(encoding="utf-8")
        server = (root / "dist" / "app.py").read_text(encoding="utf-8")

        self.assertIn('if self.path == "/health":', server)
        self.assertIn('self.send_response(200)', server)
        self.assertIn('savedMessage === "Saved" ? "Save failed" : "Saved"', page)


if __name__ == "__main__":
    unittest.main()
