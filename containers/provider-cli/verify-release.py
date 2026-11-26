"""Verify one retained OCI candidate against the reviewed dual-platform release."""
import argparse
import hashlib
import json
from pathlib import Path
import re


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read(path):
    return json.loads(path.read_bytes())


def digest(path):
    with path.open("rb") as stream:
        return "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()


def blob(layout, descriptor):
    key = descriptor["digest"]
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", key), "invalid OCI descriptor digest")
    path = layout / "blobs" / "sha256" / key[7:]
    require(path.stat().st_size == descriptor["size"] and digest(path) == key,
            "OCI descriptor size or digest mismatch")
    return path


def verify_source(lock, run):
    require(lock["schemaVersion"] == 1 and set(lock["platforms"]) == {"linux/amd64", "linux/arm64"},
            "release requires both Linux architectures")
    require(type(lock["runId"]) is int and lock["runId"] > 0 and
            type(lock["runAttempt"]) is int and lock["runAttempt"] > 0, "invalid release run")
    require(re.fullmatch(r"[0-9a-f]{40}", lock["sourceRevision"]), "invalid source revision")
    require(run["id"] == lock["runId"] and run["run_attempt"] == lock["runAttempt"] and
            run["head_sha"] == lock["sourceRevision"] and run["head_branch"] == "main" and
            run["head_repository"]["full_name"] == "zbarno/kool.ade" and
            run["path"] == ".github/workflows/provider-cli-image.yml" and
            run["event"] == "workflow_dispatch" and run["status"] == "completed" and run["conclusion"] == "success",
            "candidate must come from the specified successful main workflow run and attempt")


def verify(lock, run, directory, architecture, repo=Path(".")):
    verify_source(lock, run)
    platform = "linux/" + architecture
    approved = lock["platforms"][platform]
    require(not any(p.is_symlink() for p in directory.rglob("*")), "unexpected linked artifact")
    candidate = read(directory / "candidate.json")
    require(candidate["schemaVersion"] == 1 and candidate["sourceRevision"] == run["head_sha"] and
            candidate["runId"] == str(run["id"]) and candidate["runAttempt"] == str(run["run_attempt"]) and
            candidate["platform"] == platform and
            candidate["checks"] == ["native-cli-startup", "synthetic-hook-container"],
            "candidate smoke evidence mismatch")
    require(digest(directory / "candidate.json") == approved["candidateDigest"],
            "candidate record differs from reviewed evidence")
    require(digest(directory / "sbom.spdx.json") == candidate["sbomDigest"], "SBOM digest mismatch")
    require(digest(repo / "containers/provider-cli/Dockerfile") == candidate["dockerfileDigest"] and
            digest(repo / "containers/provider-cli/packages.lock.json") == candidate["packageLockDigest"],
            "provider recipe or packages changed since smoke")
    base_images = read(repo / "containers/base-images.lock.json")["profiles"]["node"]
    require([b["reference"] for b in base_images if b["platform"] == platform] == [candidate["baseImage"]],
            "provider base image changed since smoke")
    layout = directory / "oci"
    manifests = read(layout / "index.json")["manifests"]
    require(len(manifests) == 1, "expected one OCI manifest")
    manifest = blob(layout, manifests[0])
    require(manifests[0]["digest"] == candidate["manifestDigest"] == approved["manifestDigest"] ==
            digest(directory / "manifest.json"), "reviewed image digest mismatch")
    image = read(manifest)
    config = read(blob(layout, image["config"]))
    require(config["os"] == "linux" and config["architecture"] == architecture and
            config["config"]["User"] == "10001:10001", "image configuration mismatch")
    for layer in image["layers"]:
        blob(layout, layer)
    return approved["manifestDigest"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lock", type=Path)
    parser.add_argument("run", type=Path)
    parser.add_argument("--source-only", action="store_true")
    parser.add_argument("directory", type=Path, nargs="?")
    parser.add_argument("architecture", choices=["amd64", "arm64"], nargs="?")
    args = parser.parse_args()
    if args.source_only:
        verify_source(read(args.lock), read(args.run))
    elif args.directory and args.architecture:
        print(verify(read(args.lock), read(args.run), args.directory, args.architecture))
    else:
        parser.error("provide a candidate directory and architecture, or use --source-only")
