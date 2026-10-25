from pathlib import Path
import unittest


class BuiltHeadlessServiceTests(unittest.TestCase):
    def test_build_emits_the_approved_health_and_save_service(self):
        app = (Path(__file__).parent / "dist" / "app.py").read_text(encoding="utf-8")
        self.assertIn('self.path != "/health"', app)
        self.assertIn('self.path != "/save"', app)
        self.assertIn('{"saved": True}', app)


if __name__ == "__main__":
    unittest.main()
