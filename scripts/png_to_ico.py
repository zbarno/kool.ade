#!/usr/bin/env python3
"""Wrap one PNG image in a Windows ICO container without changing its pixels."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
ICO_HEADER_SIZE = 6
ICO_ENTRY_SIZE = 16


def convert(source: Path, destination: Path) -> None:
    data = source.read_bytes()
    if not data.startswith(PNG_SIGNATURE) or len(data) < 24:
        raise ValueError("input is not a complete PNG")
    width, height = struct.unpack(">II", data[16:24])
    if not 1 <= width <= 256 or not 1 <= height <= 256:
        raise ValueError("Windows PNG icons must be between 1 and 256 pixels per side")
    width_byte = 0 if width == 256 else width
    height_byte = 0 if height == 256 else height
    image_offset = ICO_HEADER_SIZE + ICO_ENTRY_SIZE
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack(
        "<BBBBHHII",
        width_byte,
        height_byte,
        0,
        0,
        1,
        32,
        len(data),
        image_offset,
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(header + entry + data)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        convert(args.source, args.destination)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
