#!/usr/bin/env python3
"""Check public Computer Use status copy against checked-in release flags."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
DOCS = (
    "docs/computer-use-architecture.md",
    "docs/computer-use-architecture-v1.md",
    "docs/computer-use-readiness-v1.md",
    "docs/computer-use-threat-model-v1.md",
    "docs/runtime-qa-operations.md",
    "README.md",
    "site/architecture.html",
)


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def validate_status(
    image_lock: dict[str, Any],
    catalog: dict[str, Any],
    runtime_lock: dict[str, Any],
    documents: dict[str, str],
) -> list[str]:
    errors: list[str] = []
    guide = documents.get("docs/computer-use-architecture.md", "")
    readiness = documents.get("docs/computer-use-readiness-v1.md", "")
    old_architecture = documents.get("docs/computer-use-architecture-v1.md", "")
    runtime_guide = documents.get("docs/runtime-qa-operations.md", "")
    readme = documents.get("README.md", "")
    marketing = documents.get("site/architecture.html", "")

    if image_lock.get("status") == "promoted":
        for path, content in (
            ("docs/computer-use-architecture.md", guide),
            ("docs/computer-use-architecture-v1.md", old_architecture),
        ):
            if "promoted" not in content:
                errors.append(f"{path} does not describe the promoted guest image")
        if "not_promoted` and empty" in readiness:
            errors.append("readiness guide still claims the bundled image is not promoted")
        if "guest image is published" not in readme:
            errors.append("README does not reflect the promoted guest image")
        if "promoted signed x86_64 guest image" not in marketing:
            errors.append("marketing architecture page does not reflect the promoted image")
    elif image_lock.get("status") == "not_promoted":
        if "not promoted" not in guide.lower() and "not_promoted" not in guide:
            errors.append("operator guide does not identify the guest image as unpromoted")
        if "guest image is published" in readme or "promoted signed x86_64 guest image" in marketing:
            errors.append("README or marketing page claims an image is published before promotion")
    else:
        errors.append("guest image lock has no recognized promoted status")

    images = catalog.get("images")
    protocol = None
    if isinstance(images, list) and images:
        protocol = images[0].get("companion_protocol_version")
    if protocol == 1:
        if "protocol v1" not in guide or "InternetResearch" not in guide:
            errors.append("operator guide does not match the v1 offline guest protocol")
        if "protocol v1" not in old_architecture:
            errors.append("v1 architecture page omits the currently published guest protocol")
    elif not isinstance(protocol, int) or protocol < 1:
        errors.append("signed guest catalog has no supported companion protocol version")
    else:
        for path, content in (
            ("docs/computer-use-architecture.md", guide),
            ("docs/computer-use-architecture-v1.md", old_architecture),
        ):
            if "current signed guest image uses companion protocol v1" in content:
                errors.append(f"{path} still claims the current image uses protocol v1")

    profiles = runtime_lock.get("profiles", {})
    qa_variants = [
        variant
        for profile in ("browser_qa", "desktop_qa")
        for variant in profiles.get(profile, [])
    ]
    if qa_variants and not any(item.get("runtime_enabled") is True for item in qa_variants):
        if "Production Runtime QA remains **Unavailable**" not in runtime_guide:
            errors.append("Runtime QA guide does not match disabled runtime_enabled flags")
        if "profiles remain disabled" not in documents.get("README.md", ""):
            errors.append("README does not disclose disabled Runtime QA profiles")
    elif qa_variants:
        if "unavailable" in runtime_guide.lower():
            errors.append("Runtime QA guide still says unavailable after a profile is enabled")
        if "profiles remain disabled" in readme:
            errors.append("README still says QA profiles are disabled after a profile is enabled")
    return errors


def validate_repository(root: Path = ROOT) -> list[str]:
    documents = {
        name: (root / name).read_text(encoding="utf-8")
        for name in DOCS
    }
    return validate_status(
        load_json(root / "containers/computer-use/images.lock.json"),
        load_json(root / "containers/computer-use/catalog-v1.json"),
        load_json(root / "containers/images.lock.json"),
        documents,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()
    errors = validate_repository(args.root)
    if errors:
        print("\n".join(errors))
        return 1
    print("Computer Use docs match the guest catalog and QA release flags")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
