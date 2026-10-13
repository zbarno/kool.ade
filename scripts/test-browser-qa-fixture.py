#!/usr/bin/env python3
"""Exercise the real pinned Playwright driver against the isolated Python fixture."""

from __future__ import annotations

import base64
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DRIVER = "/opt/koolade-runtime-qa/driver.mjs"
RELAY = "/opt/koolade-runtime-qa/relay.mjs"
ORIGIN = "http://qa-preview:8080"
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")


def docker(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(["docker", *args], text=True, capture_output=True, check=False)
    if check and result.returncode:
        raise RuntimeError(f"docker {' '.join(args[:4])} failed: {result.stderr[-2000:]}")
    return result


def request(actions: list[dict], *, width: int = 1280, height: int = 800, scheme: str = "light") -> dict:
    return {
        "schema_version": 1,
        "qa_run_id": str(uuid.uuid4()),
        "scenario_id": str(uuid.uuid4()),
        "task_uid": str(uuid.uuid4()),
        "code_sha": "a" * 40,
        "build_sha256": "b" * 64,
        "approved_spec_hash": "c" * 64,
        "driver_profile_version": "browser-qa-fixture-v1",
        "origin": ORIGIN,
        "viewport": {"width": width, "height": height},
        "locale": "en-US",
        "timezone": "UTC",
        "color_scheme": scheme,
        "max_screenshots": 16,
        "max_evidence_bytes": 64 * 1024 * 1024,
        "timeout_ms": 60_000,
        "actions": actions,
    }


def action(name: str, **values: object) -> dict:
    return {"action": name, **values}


def execute(image: str, network: str, value: dict, directory: Path) -> tuple[int, dict]:
    path = directory / f"{value['qa_run_id']}.json"
    path.write_text(json.dumps(value), encoding="utf-8")
    result = docker(
        "run", "--rm", "--network", network, "--read-only", "--cap-drop", "ALL",
        "--security-opt", "no-new-privileges", "--pids-limit", "128", "--memory", "1g",
        "--cpus", "2", "--tmpfs", "/tmp:rw,nosuid,nodev,noexec,size=64m",
        "--tmpfs", "/run:rw,nosuid,nodev,noexec,size=1m",
        "--mount", f"type=bind,src={path},dst=/run/koolade-runtime-qa/request.json,readonly",
        "--env", "HOME=/tmp", "--env", "CI=true", "--entrypoint", "node", image, DRIVER,
        check=False,
    )
    if not result.stdout.strip():
        return result.returncode, {"driver_error": result.stderr[-2000:]}
    return result.returncode, json.loads(result.stdout.strip().splitlines()[-1])


def assert_screenshots(report: dict, minimum: int) -> None:
    screenshots = report.get("screenshots", [])
    assert len(screenshots) >= minimum, f"expected {minimum} screenshots, found {len(screenshots)}"
    for screenshot in screenshots:
        payload = base64.b64decode(screenshot["bytes_base64"], validate=True)
        assert screenshot["mime"] == "image/png"
        assert hashlib.sha256(payload).hexdigest() == screenshot["sha256"]
        assert payload.startswith(b"\x89PNG\r\n\x1a\n")


def main() -> None:
    image = sys.argv[1] if len(sys.argv) == 2 else ""
    if not image:
        raise SystemExit("usage: test-browser-qa-fixture.py IMAGE")
    suffix = uuid.uuid4().hex[:12]
    task_network = f"koolade-qa-fixture-task-{suffix}"
    browser_network = f"koolade-qa-fixture-browser-{suffix}"
    task = f"koolade-qa-fixture-service-{suffix}"
    relay = f"koolade-qa-fixture-relay-{suffix}"
    docker("network", "create", "--internal", task_network)
    docker("network", "create", "--internal", browser_network)
    try:
        fixture = ROOT / "tests/fixtures/runtime_qa/browser/server.py"
        docker(
            "run", "--detach", "--name", task, "--network", task_network,
            "--network-alias", "qa-task", "--read-only", "--cap-drop", "ALL",
            "--security-opt", "no-new-privileges", "--pids-limit", "32", "--memory", "256m",
            "--tmpfs", "/tmp:rw,nosuid,nodev,noexec,size=16m",
            "--tmpfs", "/workspace:rw,nosuid,nodev,noexec,size=16m",
            "--mount", f"type=bind,src={fixture},dst=/workspace/server.py,readonly",
            "--env", "QA_FIXTURE_HOST=0.0.0.0", "--env", "QA_FIXTURE_PORT=4173",
            "--entrypoint", "python3", image, "/workspace/server.py",
        )
        docker(
            "run", "--detach", "--name", relay, "--network", task_network,
            "--network-alias", "qa-relay", "--read-only", "--cap-drop", "ALL",
            "--security-opt", "no-new-privileges", "--pids-limit", "64", "--memory", "256m",
            "--tmpfs", "/tmp:rw,nosuid,nodev,noexec,size=16m", "--env", "HOME=/tmp",
            "--env", "QA_TARGET_PORT=4173", "--env", "QA_HEALTH_PATH=/",
            "--entrypoint", "node", image, RELAY,
        )
        docker("network", "connect", "--alias", "qa-preview", browser_network, relay)
        deadline = time.monotonic() + 60
        while True:
            probe = docker(
                "run", "--rm", "--network", browser_network, "--read-only",
                "--cap-drop", "ALL", "--tmpfs", "/tmp:rw,nosuid,nodev,noexec,size=16m",
                "--entrypoint", "node", image, "-e",
                "fetch('http://qa-preview:8080/__koolade_health').then(r=>process.exit(r.status===204?0:1)).catch(()=>process.exit(1))",
                check=False,
            )
            if probe.returncode == 0:
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("isolated Python preview relay did not become ready")
            time.sleep(0.25)

        with tempfile.TemporaryDirectory(prefix="koolade-browser-qa-") as temporary:
            directory = Path(temporary)
            passed_actions = [
                action("browser_open", url="http://qa-service/", screenshot_checkpoint=True),
                action("browser_fill", locator={"locator": "label", "label": "Message"}, value="synthetic message"),
                action("browser_press", locator={"locator": "label", "label": "Message"}, key="Enter"),
                action("browser_assert_text", locator={"locator": "test_id", "test_id": "status"}, expected_text="Saved"),
                action("browser_click", locator={"locator": "role", "role": "link", "name": "Settings"}),
                action("browser_assert_url", url=f"{ORIGIN}/?page=settings"),
                action("browser_click", locator={"locator": "role", "role": "button", "name": "Open details"}),
                action("browser_assert_text", locator={"locator": "test_id", "test_id": "details-dialog"}, expected_text="Details are available"),
                action("browser_click", locator={"locator": "role", "role": "button", "name": "Close"}),
                action("browser_set_viewport", width=390, height=844),
                action("browser_screenshot"),
            ]
            code, report = execute(image, browser_network, request(passed_actions), directory)
            assert code == 0 and report["outcome"] == "completed", report
            assert len(report["actions"]) == len(passed_actions)
            assert report["actions"][5]["post_state"]["url"] == f"{ORIGIN}/?page=settings"
            assert_screenshots(report, 3)

            js_error = request([action("browser_open", url="http://qa-service/?defect=js-error")])
            code, report = execute(image, browser_network, js_error, directory)
            assert code == 0 and report["outcome"] == "completed", report
            assert any("seeded fixture exception" in message for message in report["console_errors"])

            dark = request([action("browser_open", url="http://qa-service/", screenshot_checkpoint=True)], scheme="dark")
            code, report = execute(image, browser_network, dark, directory)
            assert code == 0 and report["outcome"] == "completed", report
            assert_screenshots(report, 1)

            failed = request([
                action("browser_open", url="http://qa-service/"),
                action("browser_assert_text", locator={"locator": "test_id", "test_id": "status"}, expected_text="not present"),
            ])
            code, report = execute(image, browser_network, failed, directory)
            assert code == 1 and report["outcome"] == "failed"
            assert report["actions"][-1]["status"] == "failed"
            assert_screenshots(report, 1)

            escape = request([
                action("browser_open", url="http://qa-service/?defect=metadata"),
                action("browser_assert_text", locator={"locator": "test_id", "test_id": "status"}, expected_text="Request blocked"),
            ])
            code, report = execute(image, browser_network, escape, directory)
            assert code == 1 and report["outcome"] == "failed"
            assert "169.254.169.254" in (report.get("policy_violation") or ""), {
                key: report.get(key)
                for key in ("error", "policy_violation", "console_errors", "failed_requests")
            }

            redirect = request([action("browser_open", url="http://qa-service/redirect")])
            code, report = execute(image, browser_network, redirect, directory)
            assert code == 1 and report["outcome"] == "failed"
            assert "169.254.169.254" in report["policy_violation"]

            for defect, expected in [
                ("iframe", "169.254.169.254"),
                ("localhost", "127.0.0.1:4173"),
                ("alternate-port", "qa-preview:8081"),
                ("websocket", "example.com"),
            ]:
                escaped = request([action("browser_open", url=f"http://qa-service/?defect={defect}")])
                code, report = execute(image, browser_network, escaped, directory)
                assert code == 1 and report["outcome"] == "failed", report
                assert expected in report["policy_violation"], report

            for blocked_url in (
                "javascript:alert(1)",
                "file:///etc/passwd",
                "http://localhost:4173/",
                "http://qa-preview:8081/",
                "http://example.com/",
                "https://qa-preview:8080/",
            ):
                rejected = request([action("browser_open", url=blocked_url)])
                code, report = execute(image, browser_network, rejected, directory)
                assert code == 2 and "action is not allowed by browser QA policy" in report["driver_error"], report
    finally:
        docker("rm", "--force", relay, check=False)
        docker("rm", "--force", task, check=False)
        docker("network", "rm", browser_network, check=False)
        docker("network", "rm", task_network, check=False)
    print("browser QA fixture passed: Python preview, real Playwright actions, screenshots, page error evidence, assertion failure, and origin escape denials")


if __name__ == "__main__":
    main()
