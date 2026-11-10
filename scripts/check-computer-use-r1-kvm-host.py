#!/usr/bin/env python3
"""Read-only prerequisite check for a trusted R1 Linux KVM runner.

This proves host prerequisites only. It does not boot a guest or certify VM
isolation, guest actions, the UI, or any Computer Use release tier.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import stat
import subprocess
import sys
from pathlib import Path


REQUIRED_CONTROLLERS = {"cpu", "memory", "pids"}
QEMU_SYSTEM = Path("/usr/bin/qemu-system-x86_64")
QEMU_IMG = Path("/usr/bin/qemu-img")


def fail(message: str) -> None:
    raise RuntimeError(message)


def require_trusted_executable(path: Path) -> None:
    try:
        metadata = path.stat()
    except OSError as error:
        fail(f"required executable is unavailable: {path}: {error}")
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        fail(f"required executable is not a trusted root-owned system file: {path}")


def qemu_version(path: Path) -> str:
    require_trusted_executable(path)
    try:
        result = subprocess.run(
            [str(path), "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=3,
            env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        fail(f"could not safely query {path}: {error}")
    first_line = result.stdout.splitlines()[0] if result.stdout.splitlines() else ""
    if not re.match(r"^QEMU emulator version \d+\.\d+\.\d+\b", first_line):
        if path == QEMU_IMG:
            if not first_line.startswith("qemu-img version "):
                fail(f"malformed qemu-img version output: {first_line!r}")
        else:
            fail(f"malformed QEMU version output: {first_line!r}")
    return first_line[:160]


def qemu_accelerators() -> str:
    require_trusted_executable(QEMU_SYSTEM)
    try:
        result = subprocess.run(
            [str(QEMU_SYSTEM), "-accel", "help"],
            check=True,
            capture_output=True,
            text=True,
            timeout=3,
            env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        fail(f"could not safely query QEMU accelerators: {error}")
    accelerators = {line.strip() for line in result.stdout.splitlines()}
    if "kvm" not in accelerators:
        fail("QEMU does not report the KVM accelerator")
    return "kvm available"


def nested_hypervisor() -> None:
    probe = Path("/usr/bin/systemd-detect-virt")
    require_trusted_executable(probe)
    try:
        result = subprocess.run(
            [str(probe), "--vm"],
            capture_output=True,
            text=True,
            timeout=3,
            env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        fail(f"could not safely check nested virtualization: {error}")
    if (result.returncode, result.stdout.strip()) != (1, "none"):
        fail("runner is virtualized or nested-virtualization status is unknown")


def cgroup_root(
    proc_path: Path = Path("/proc/self/cgroup"),
    mount_root: Path = Path("/sys/fs/cgroup"),
) -> Path:
    try:
        records = [
            line.split(":", 2)[2]
            for line in proc_path.read_text().splitlines()
            if line.startswith("0::")
        ]
    except (OSError, IndexError) as error:
        fail(f"unified cgroup path is unavailable: {error}")
    if len(records) != 1 or not records[0].startswith("/"):
        fail("process does not have exactly one unified cgroup path")
    relative = Path(records[0].lstrip("/"))
    if any(part in {"..", "."} for part in relative.parts):
        fail("unified cgroup path contains an unsafe component")
    root = mount_root / relative
    try:
        if root.resolve(strict=True) != root or not root.is_dir():
            fail("unified cgroup path is not a canonical directory")
        available = set((root / "cgroup.controllers").read_text().split())
        enabled = {value.lstrip("+") for value in (root / "cgroup.subtree_control").read_text().split()}
    except OSError as error:
        fail(f"delegated cgroup controls are unavailable: {error}")
    if not REQUIRED_CONTROLLERS <= available or not REQUIRED_CONTROLLERS <= enabled:
        fail("delegated cgroup v2 cpu, memory, and pids controllers are not enabled")
    if not (root / "cgroup.procs").is_file():
        fail("delegated cgroup v2 process control is unavailable")
    if not os.access(root, os.W_OK | os.X_OK):
        fail("delegated cgroup directory is not writable by the runner account")
    return root


def check() -> dict[str, object]:
    if platform.system() != "Linux" or platform.machine() not in {"x86_64", "amd64"}:
        fail("R1 local KVM preflight requires Linux x86_64")
    if os.geteuid() == 0:
        fail("QEMU runner must be an unprivileged user")
    nested_hypervisor()
    try:
        fd = os.open("/dev/kvm", os.O_RDWR | os.O_CLOEXEC)
    except OSError as error:
        fail(f"/dev/kvm is not accessible for read/write: {error}")
    else:
        os.close(fd)
    return {
        "host": "Linux x86_64",
        "qemu_system": qemu_version(QEMU_SYSTEM),
        "qemu_kvm_accelerator": qemu_accelerators(),
        "qemu_img": qemu_version(QEMU_IMG),
        "kvm_device": "read-write accessible",
        "delegated_cgroup_v2": "cpu, memory, and pids controllers delegated",
        "scope": "host prerequisites only; no guest boot or isolation acceptance",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--github-summary", type=Path)
    args = parser.parse_args()
    try:
        evidence = check()
    except RuntimeError as error:
        print(f"Computer Use R1 host preflight failed: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps(evidence, indent=2, sort_keys=True)
    print(rendered)
    if args.github_summary:
        with args.github_summary.open("a", encoding="utf-8") as output:
            output.write("## Computer Use R1 KVM host preflight\n\n")
            output.write("Host prerequisites passed. This is not guest boot/isolation acceptance.\n\n")
            output.write("```json\n" + rendered + "\n```\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
