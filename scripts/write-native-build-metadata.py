#!/usr/bin/env python3
"""Write traceable, non-secret metadata beside a native CI package."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    hasher = hashlib.sha256()
    with args.artifact.open("rb") as artifact:
        for chunk in iter(lambda: artifact.read(1024 * 1024), b""):
            hasher.update(chunk)
    digest = hasher.hexdigest()
    metadata = {
        "artifact": args.artifact.name,
        "artifact_sha256": digest,
        "commit": os.environ.get("GITHUB_SHA", "local"),
        "runner_os": os.environ.get("RUNNER_OS", "local"),
        "target": args.target,
        "version": args.version,
        "distribution_status": "unsigned-ci-artifact",
        "runtime_support_claim": False,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
