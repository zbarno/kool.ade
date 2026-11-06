#!/usr/bin/env python3
"""Create deterministic CycloneDX inventory for installed Debian packages."""
import hashlib
import json
import sys
import uuid
from urllib.parse import quote
from pathlib import Path

source, destination = map(Path, sys.argv[1:3])
records = []
for line in source.read_text(encoding="utf-8").splitlines():
    name, version = line.split("\t", 1)
    records.append(
        {
            "type": "library",
            "name": name,
            "version": version,
            "purl": f"pkg:deb/debian/{quote(name, safe='.-_')}@{quote(version, safe='.-_~')}?arch=amd64",
        }
    )
records.sort(key=lambda record: (record["name"], record["version"]))
payload = json.dumps(records, separators=(",", ":"), sort_keys=True).encode()
serial = uuid.uuid5(uuid.NAMESPACE_URL, "urn:sha256:" + hashlib.sha256(payload).hexdigest())
bom = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "serialNumber": f"urn:uuid:{serial}",
    "version": 1,
    "components": records,
}
destination.write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n", encoding="utf-8")
