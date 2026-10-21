"""Rootless Docker checks for bounded headless-service API QA."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Callable


def test_service_driver(
    image: str,
    network: str,
    directory: Path,
    execute: Callable,
    request: Callable,
    action: Callable,
) -> None:
    """Cover API relay forwarding, body redaction, status evidence, and URL policy."""
    value = request([action(
        "api_request", method="POST", path="/save",
        json_body=json.dumps({"message": "synthetic API fixture"}),
        expected_status=200, expected_body_contains='"saved":true',
    )])
    value["max_screenshots"] = 0
    code, report = execute(image, network, value, directory)
    assert code == 0 and report["outcome"] == "completed", report
    assert report["screenshots"] == []
    assert report["actions"][0]["response_status"] == 200
    assert report["actions"][0]["body_matches"] is True
    assert "saved" not in json.dumps(report), "API response body leaked into evidence"

    value = request([action(
        "api_request", method="GET", path="http://example.com/", expected_status=200,
    )])
    value["max_screenshots"] = 0
    code, report = execute(image, network, value, directory)
    assert code == 2 and "action is not allowed" in report["driver_error"], report
