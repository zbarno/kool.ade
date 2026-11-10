import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "evaluations/manager_knowledge.py"
SPEC = importlib.util.spec_from_file_location("manager_knowledge", SCRIPT)
manager_knowledge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(manager_knowledge)


def result(question_id, stage, **overrides):
    row = {
        "question_id": question_id,
        "stage": stage,
        "judgment_source": "deterministic_rule",
        "grounded_correct": True,
        "unsupported_implementation_claim": False,
        "unsupported_approval_claim": False,
        "asked_human": False,
        "latency_ms": 20,
        "input_tokens": 100,
        "output_tokens": 30,
        "cached_input_tokens": 0,
        "cost_usd": 0.001,
        "profile_cache_hits": 0,
        "profile_refreshes": 1,
        "scanned_bytes": 2048,
        "omitted_sources": 0,
    }
    row.update(overrides)
    return row


class ManagerKnowledgeEvaluationTests(unittest.TestCase):
    def test_question_set_has_required_size_and_non_product_repositories(self):
        questions = manager_knowledge.load_questions()
        self.assertGreaterEqual(len(questions), 40)
        self.assertGreaterEqual(
            len({row["fixture"] for row in questions} - {"koolade-rust"}), 2
        )
        self.assertGreaterEqual(len({row["category"] for row in questions}), 8)

    def test_score_reports_accuracy_safety_questions_latency_and_cost(self):
        questions = manager_knowledge.load_questions()
        before = {row["id"]: result(row["id"], "before", grounded_correct=False) for row in questions}
        after = {row["id"]: result(row["id"], "after") for row in questions}
        answerable = next(row for row in questions if row["answerable"])
        before[answerable["id"]]["asked_human"] = True
        critical = next(row for row in questions if row["critical"])
        after[critical["id"]]["unsupported_approval_claim"] = False
        report = manager_knowledge.score(questions, before, after)
        self.assertEqual(report["question_count"], 44)
        self.assertEqual(report["before"]["accuracy"], 0)
        self.assertEqual(report["after"]["accuracy"], 1)
        self.assertTrue(report["meets_targets"])
        self.assertEqual(report["before_unnecessary_human_questions"], 1)
        self.assertEqual(report["after_unnecessary_human_questions"], 0)
        answerable_count = report["answerable_question_count"]
        self.assertEqual(report["before_unnecessary_human_question_rate"], 1 / answerable_count)
        self.assertEqual(report["after_unnecessary_human_question_rate"], 0)
        self.assertEqual(
            report["unnecessary_human_question_rate_change"], -1 / answerable_count
        )
        self.assertEqual(report["latency_ms"]["p95"], 20)
        self.assertGreater(report["totals"]["cost_usd"], 0)

    def test_critical_unsupported_claim_fails_target(self):
        questions = manager_knowledge.load_questions()
        before = {row["id"]: result(row["id"], "before") for row in questions}
        after = {row["id"]: result(row["id"], "after") for row in questions}
        critical = next(row for row in questions if row["critical"])
        after[critical["id"]]["unsupported_implementation_claim"] = True
        report = manager_knowledge.score(questions, before, after)
        self.assertEqual(report["critical_unsupported_claims"], 1)
        self.assertFalse(report["meets_targets"])

    def test_results_must_be_reviewed_not_model_self_graded(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.jsonl"
            path.write_text(json.dumps(result("Q01", "after", judgment_source="model")))
            with self.assertRaises(manager_knowledge.EvaluationError):
                manager_knowledge.load_results(path, "after")

    def test_stage_and_question_identity_are_enforced(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.jsonl"
            path.write_text(json.dumps(result("Q01", "before")))
            with self.assertRaises(manager_knowledge.EvaluationError):
                manager_knowledge.load_results(path, "after")


if __name__ == "__main__":
    unittest.main()
