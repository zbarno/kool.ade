#!/usr/bin/env python3
"""Score human-reviewed Manager knowledge benchmark result files."""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_QUESTIONS = ROOT / "tests/fixtures/manager-knowledge-questions-v1.json"
VALID_STAGES = {"before", "after"}
VALID_JUDGMENTS = {"human", "deterministic_rule"}
REQUIRED_QUESTION_FIELDS = {
    "id", "category", "fixture", "question", "expected", "sources", "answerable", "critical"
}


class EvaluationError(ValueError):
    pass


def load_questions(path: Path = DEFAULT_QUESTIONS) -> list[dict[str, Any]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    questions = data.get("questions")
    if data.get("version") != 1 or not isinstance(questions, list):
        raise EvaluationError("unsupported or malformed question set")
    ids = [row.get("id") for row in questions]
    if len(questions) < 40 or len(set(ids)) != len(ids) or any(not item for item in ids):
        raise EvaluationError("question set needs at least 40 unique, named cases")
    fixtures = {row.get("fixture") for row in questions}
    if len(fixtures - {"koolade-rust"}) < 2:
        raise EvaluationError("benchmark needs at least two non-Kool.ad/e fixtures")
    fixture_root = ROOT / "tests/fixtures/manager-knowledge"
    for fixture in fixtures - {"koolade-rust"}:
        if not (fixture_root / fixture).is_dir():
            raise EvaluationError(f"missing repository fixture: {fixture}")
    for row in questions:
        if not REQUIRED_QUESTION_FIELDS.issubset(row):
            raise EvaluationError(f"question {row.get('id')} is missing required rubric fields")
        if not isinstance(row["sources"], list) or not row["sources"]:
            raise EvaluationError(f"question {row['id']} needs expected evidence classes")
        if not isinstance(row["answerable"], bool) or not isinstance(row["critical"], bool):
            raise EvaluationError(f"question {row['id']} needs boolean answerable/critical labels")
    return questions


def load_results(path: Path, stage: str) -> dict[str, dict[str, Any]]:
    if stage not in VALID_STAGES:
        raise EvaluationError(f"invalid evaluation stage: {stage}")
    rows: dict[str, dict[str, Any]] = {}
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise EvaluationError(f"{path}:{line_number}: invalid JSON: {error}") from error
        question_id = row.get("question_id")
        if not isinstance(question_id, str) or question_id in rows:
            raise EvaluationError(f"{path}:{line_number}: missing or duplicate question_id")
        if row.get("stage") != stage:
            raise EvaluationError(f"{path}:{line_number}: expected stage {stage}")
        if row.get("judgment_source") not in VALID_JUDGMENTS:
            raise EvaluationError(
                f"{path}:{line_number}: judgments must be human-reviewed or deterministic"
            )
        for key in (
            "grounded_correct",
            "unsupported_implementation_claim",
            "unsupported_approval_claim",
            "asked_human",
        ):
            if not isinstance(row.get(key), bool):
                raise EvaluationError(f"{path}:{line_number}: {key} must be boolean")
        for key in (
            "latency_ms",
            "input_tokens",
            "output_tokens",
            "cached_input_tokens",
            "cost_usd",
            "profile_cache_hits",
            "profile_refreshes",
            "scanned_bytes",
            "omitted_sources",
        ):
            value = row.get(key)
            if (
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or not math.isfinite(value)
                or value < 0
            ):
                raise EvaluationError(f"{path}:{line_number}: {key} must be nonnegative")
        rows[question_id] = row
    return rows


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    return ordered[max(0, math.ceil(fraction * len(ordered)) - 1)]


def score(
    questions: list[dict[str, Any]],
    before: dict[str, dict[str, Any]],
    after: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    expected_ids = {question["id"] for question in questions}
    for stage, rows in (("before", before), ("after", after)):
        if set(rows) != expected_ids:
            missing = sorted(expected_ids - set(rows))
            extra = sorted(set(rows) - expected_ids)
            raise EvaluationError(f"{stage} results do not match question set; missing={missing}, extra={extra}")

    categories: dict[str, dict[str, Any]] = defaultdict(lambda: {"count": 0, "correct": 0})
    after_correct = 0
    critical_unsupported = 0
    answerable_count = 0
    unnecessary_questions = 0
    latencies: list[float] = []
    totals = {key: 0.0 for key in (
        "input_tokens", "output_tokens", "cached_input_tokens", "cost_usd",
        "profile_cache_hits", "profile_refreshes", "scanned_bytes", "omitted_sources",
    )}
    for question in questions:
        result = after[question["id"]]
        category = categories[question["category"]]
        category["count"] += 1
        category["correct"] += int(result["grounded_correct"])
        after_correct += int(result["grounded_correct"])
        if question["critical"]:
            critical_unsupported += int(result["unsupported_implementation_claim"])
            critical_unsupported += int(result["unsupported_approval_claim"])
        if question["answerable"]:
            answerable_count += 1
            unnecessary_questions += int(result["asked_human"])
        latencies.append(float(result["latency_ms"]))
        for key in totals:
            totals[key] += float(result[key])

    before_correct = sum(int(row["grounded_correct"]) for row in before.values())
    count = len(questions)
    accuracy = after_correct / count
    report = {
        "question_count": count,
        "fixture_count": len({question["fixture"] for question in questions}),
        "before": {"grounded_correct": before_correct, "accuracy": before_correct / count},
        "after": {"grounded_correct": after_correct, "accuracy": accuracy},
        "category_accuracy": {
            name: {"count": value["count"], "accuracy": value["correct"] / value["count"]}
            for name, value in sorted(categories.items())
        },
        "critical_unsupported_claims": critical_unsupported,
        "unnecessary_human_questions": unnecessary_questions,
        "answerable_question_count": answerable_count,
        "unnecessary_human_question_rate": unnecessary_questions / max(1, answerable_count),
        "latency_ms": {"p50": percentile(latencies, 0.50), "p95": percentile(latencies, 0.95)},
        "totals": totals,
        "meets_targets": accuracy >= 0.90 and critical_unsupported == 0,
    }
    report["accuracy_change"] = report["after"]["accuracy"] - report["before"]["accuracy"]
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path, help="JSONL baseline result file")
    parser.add_argument("--after", required=True, type=Path, help="JSONL current result file")
    parser.add_argument("--questions", type=Path, default=DEFAULT_QUESTIONS)
    args = parser.parse_args()
    try:
        report = score(
            load_questions(args.questions),
            load_results(args.before, "before"),
            load_results(args.after, "after"),
        )
    except (EvaluationError, OSError, json.JSONDecodeError) as error:
        print(f"evaluation error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["meets_targets"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
