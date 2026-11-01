
import tempfile
import unittest
from pathlib import Path

from bio_errata_check import (
    scan_linked_code,
    check_linked_binary,
)


def w(value):
    return value.to_bytes(4, "little")


class BioErrataTests(unittest.TestCase):

    def test_real_jal_bug2(self):
        self.assertEqual(
            scan_linked_code(w(0xD54FF0EF), 4),
            [(0, 0xD54FF0EF, "BUG2 phantom-rs2")],
        )

    def test_safe_jals(self):
        image = w(0xD3EFF0EF) + w(0xD2AFF0EF)
        self.assertEqual(scan_linked_code(image, 8), [])

    def test_mixed_instruction_lengths(self):
        image = b"\x01\x00" + w(0xD54FF0EF)
        self.assertEqual(
            scan_linked_code(image, 6),
            [(2, 0xD54FF0EF, "BUG2 phantom-rs2")],
        )

    def test_bug1(self):
        self.assertEqual(
            scan_linked_code(w(0x00080037), 4),
            [(0, 0x00080037, "BUG1 phantom-rs1")],
        )

    def test_i_type_bug2(self):
        self.assertEqual(
            scan_linked_code(w(0x01400513), 4),
            [(0, 0x01400513, "BUG2 phantom-rs2")],
        )

    def test_rsb_are_not_bug2(self):
        image = b"".join(map(
            w, [0x01400033, 0x01400023, 0x01400063]
        ))
        self.assertEqual(scan_linked_code(image, 12), [])

    def test_appended_data_excluded(self):
        image = b"\x01\x00\x00\x00" + w(0xD54FF0EF)
        self.assertEqual(scan_linked_code(image, 2), [])

    def test_invalid_extents(self):
        for end in (0, 3, 6):
            with self.subTest(end=end):
                with self.assertRaises(ValueError):
                    scan_linked_code(w(0xD54FF0EF), end)

    def test_truncated_instruction(self):
        with self.assertRaisesRegex(ValueError, "Truncated"):
            scan_linked_code(b"\xef\xf0", 2)

    def test_long_instruction(self):
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            scan_linked_code(b"\x1f\x00", 2)

    def test_invalid_footer(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "image.bin"

            binary.write_bytes(b"\x01\x00")
            with self.assertRaisesRegex(ValueError, "footer"):
                check_linked_binary(binary)

            binary.write_bytes(b"\x01\x00" + w(3))
            with self.assertRaisesRegex(
                ValueError, "Invalid executable extent"
            ):
                check_linked_binary(binary)

    def test_integration_of_binary_footer(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "image.bin"
            source_image = b"\x01\x00" + w(0xD54FF0EF)

            # The dangerous word is data, outside the executable extent.
            binary.write_bytes(source_image + w(2))
            self.assertEqual(check_linked_binary(binary), 2)
            self.assertEqual(binary.read_bytes(), source_image)

            # The same word is executable and must be rejected.
            binary.write_bytes(source_image + w(6))
            with self.assertRaisesRegex(ValueError, "BUG2"):
                check_linked_binary(binary)


if __name__ == "__main__":
    unittest.main()
