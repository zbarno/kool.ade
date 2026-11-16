#!/usr/bin/env python3
"""Set FAT32 directory-entry timestamps to stable DOS epoch values."""

import os
import sys
from pathlib import Path


DOS_EPOCH_DATE = 0x0021  # 1980-01-01
DOS_EPOCH_TIME = 0
FAT32_END_OF_CHAIN = 0x0FFFFFF8
FAT32_BAD_CLUSTER = 0x0FFFFFF7


def normalize(image: Path) -> None:
    with image.open("r+b", buffering=0) as disk:
        size = os.fstat(disk.fileno()).st_size
        boot = disk.read(512)
        if len(boot) != 512 or boot[510:512] != b"\x55\xaa" or boot[82:90] != b"FAT32   ":
            raise ValueError("invalid FAT boot sector")
        bytes_per_sector = int.from_bytes(boot[11:13], "little")
        sectors_per_cluster = boot[13]
        reserved_sectors = int.from_bytes(boot[14:16], "little")
        fat_count = boot[16]
        sectors_per_fat = int.from_bytes(boot[36:40], "little")
        root_cluster = int.from_bytes(boot[44:48], "little") & 0x0FFFFFFF
        if (
            bytes_per_sector < 512
            or bytes_per_sector > 4096
            or bytes_per_sector & (bytes_per_sector - 1)
            or sectors_per_cluster == 0
            or sectors_per_cluster & (sectors_per_cluster - 1)
            or reserved_sectors == 0
            or fat_count == 0
            or sectors_per_fat == 0
            or root_cluster < 2
        ):
            raise ValueError("unsupported FAT32 geometry")

        fat_offset = reserved_sectors * bytes_per_sector
        data_offset = (reserved_sectors + fat_count * sectors_per_fat) * bytes_per_sector
        cluster_size = bytes_per_sector * sectors_per_cluster
        cluster_count = (size - data_offset) // cluster_size
        if data_offset >= size or cluster_count == 0 or root_cluster >= cluster_count + 2:
            raise ValueError("FAT32 data region is outside the image")

        def next_cluster(cluster: int) -> int:
            disk.seek(fat_offset + cluster * 4)
            entry = disk.read(4)
            if len(entry) != 4:
                raise ValueError("truncated FAT table")
            value = int.from_bytes(entry, "little") & 0x0FFFFFFF
            if value >= FAT32_END_OF_CHAIN:
                return 0
            if value == FAT32_BAD_CLUSTER or value < 2 or value >= cluster_count + 2:
                raise ValueError("invalid FAT cluster chain")
            return value

        def cluster_chain(first: int):
            visited = set()
            current = first
            while current:
                if current in visited:
                    raise ValueError("cyclic FAT cluster chain")
                visited.add(current)
                yield current
                current = next_cluster(current)

        visited_directories = set()

        def normalize_directory(first_cluster: int) -> None:
            if first_cluster in visited_directories:
                return
            visited_directories.add(first_cluster)
            for cluster in cluster_chain(first_cluster):
                offset = data_offset + (cluster - 2) * cluster_size
                disk.seek(offset)
                contents = bytearray(disk.read(cluster_size))
                if len(contents) != cluster_size:
                    raise ValueError("truncated FAT directory cluster")
                changed = False
                for position in range(0, cluster_size, 32):
                    entry = contents[position : position + 32]
                    first_byte = entry[0]
                    if first_byte == 0:
                        break
                    if first_byte == 0xE5 or entry[11] == 0x0F:
                        continue
                    entry[13] = 0
                    entry[14:16] = DOS_EPOCH_TIME.to_bytes(2, "little")
                    entry[16:18] = DOS_EPOCH_DATE.to_bytes(2, "little")
                    entry[18:20] = DOS_EPOCH_DATE.to_bytes(2, "little")
                    entry[22:24] = DOS_EPOCH_TIME.to_bytes(2, "little")
                    entry[24:26] = DOS_EPOCH_DATE.to_bytes(2, "little")
                    contents[position : position + 32] = entry
                    changed = True

                    attributes = entry[11]
                    short_name = bytes(entry[0:11])
                    if attributes & 0x10 and short_name not in (b".          ", b"..         "):
                        child_cluster = (
                            int.from_bytes(entry[20:22], "little") << 16
                        ) | int.from_bytes(entry[26:28], "little")
                        child_cluster &= 0x0FFFFFFF
                        if child_cluster >= 2:
                            normalize_directory(child_cluster)
                if changed:
                    disk.seek(offset)
                    disk.write(contents)

        normalize_directory(root_cluster)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: normalize-fat32.py EFI_PARTITION", file=sys.stderr)
        return 2
    try:
        normalize(Path(sys.argv[1]))
    except (OSError, ValueError) as error:
        print(f"FAT32 timestamp normalization failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
