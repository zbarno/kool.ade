import struct
import tempfile
import unittest
from pathlib import Path

from scripts.png_to_ico import convert


def png_header(width: int, height: int) -> bytes:
    return (
        b"\x89PNG\r\n\x1a\n"
        + struct.pack(">I", 13)
        + b"IHDR"
        + struct.pack(">II", width, height)
        + bytes([8, 6, 0, 0, 0])
    )


class PngToIcoTests(unittest.TestCase):
    def test_wraps_supported_png_bytes_without_rewriting_them(self) -> None:
        payload = png_header(128, 128) + b"synthetic-png-payload"
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.png"
            destination = Path(directory) / "nested" / "icon.ico"
            source.write_bytes(payload)

            convert(source, destination)

            data = destination.read_bytes()
        self.assertEqual(data[:6], struct.pack("<HHH", 0, 1, 1))
        self.assertEqual(data[6:22], struct.pack("<BBBBHHII", 128, 128, 0, 0, 1, 32, len(payload), 22))
        self.assertEqual(data[22:], payload)

    def test_256_pixel_icon_uses_the_reserved_zero_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.png"
            destination = Path(directory) / "icon.ico"
            source.write_bytes(png_header(256, 256))

            convert(source, destination)

            self.assertEqual(destination.read_bytes()[6:8], b"\0\0")

    def test_rejects_non_png_and_oversized_images(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.png"
            destination = root / "icon.ico"
            source.write_bytes(b"not a png")
            with self.assertRaises(ValueError):
                convert(source, destination)

            source.write_bytes(png_header(512, 512))
            with self.assertRaises(ValueError):
                convert(source, destination)


if __name__ == "__main__":
    unittest.main()
