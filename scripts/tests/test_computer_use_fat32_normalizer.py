import importlib.util
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
NORMALIZER_PATH = ROOT / "containers/computer-use/normalize-fat32.py"
SPEC = importlib.util.spec_from_file_location("normalize_fat32", NORMALIZER_PATH)
NORMALIZER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NORMALIZER)


def make_entry(name, attributes, cluster):
    entry = bytearray(32)
    entry[:11] = name
    entry[11] = attributes
    entry[13] = 99
    entry[14:16] = b"\xff\xff"
    entry[16:18] = b"\xff\xff"
    entry[18:20] = b"\xff\xff"
    entry[20:22] = (cluster >> 16).to_bytes(2, "little")
    entry[22:24] = b"\xff\xff"
    entry[24:26] = b"\xff\xff"
    entry[26:28] = (cluster & 0xFFFF).to_bytes(2, "little")
    entry[28:32] = (123).to_bytes(4, "little")
    return entry


class Fat32NormalizerTests(unittest.TestCase):
    def test_recursively_normalizes_directory_entry_times_idempotently(self):
        image = bytearray(5 * 512)
        boot = image[:512]
        boot[11:13] = (512).to_bytes(2, "little")
        boot[13] = 1
        boot[14:16] = (1).to_bytes(2, "little")
        boot[16] = 1
        boot[36:40] = (1).to_bytes(4, "little")
        boot[44:48] = (2).to_bytes(4, "little")
        boot[82:90] = b"FAT32   "
        boot[510:512] = b"\x55\xaa"
        image[:512] = boot

        fat_offset = 512
        for cluster in (0, 1, 2, 3, 4):
            value = 0x0FFFFFF8 if cluster in (0, 2, 3, 4) else 0x0FFFFFFF
            start = fat_offset + cluster * 4
            image[start : start + 4] = value.to_bytes(4, "little")

        root_offset = 1024
        image[root_offset : root_offset + 32] = make_entry(b"SUBDIR     ", 0x10, 4)
        child_offset = 1024 + (4 - 2) * 512
        image[child_offset : child_offset + 32] = make_entry(b"BOOTAA64EFI", 0x20, 3)

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "esp.img"
            path.write_bytes(image)
            NORMALIZER.normalize(path)
            normalized = path.read_bytes()
            NORMALIZER.normalize(path)
            self.assertEqual(path.read_bytes(), normalized)

        for offset in (root_offset, child_offset):
            entry = normalized[offset : offset + 32]
            self.assertEqual(entry[13], 0)
            self.assertEqual(entry[14:16], b"\x00\x00")
            self.assertEqual(entry[16:18], b"\x21\x00")
            self.assertEqual(entry[18:20], b"\x21\x00")
            self.assertEqual(entry[22:24], b"\x00\x00")
            self.assertEqual(entry[24:26], b"\x21\x00")


if __name__ == "__main__":
    unittest.main()
