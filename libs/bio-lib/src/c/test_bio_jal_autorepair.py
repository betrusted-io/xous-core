
import unittest

from clang2rustasm import (
    _symbolic_jump_direction,
    _symbolic_jal_indices,
    _insert_jal_nops,
    format_asm_output,
)


class JalAutorepairTests(unittest.TestCase):

    def test_symbolic_jumps(self):
        lines = [(s, None) for s in (
            "jal ra, 20f",
            "j 21b",
            "jal 22f",
            "jal zero, 23b",
            "j 24f",
            "jalr ra, 0(a0)",
            "jal ra, 20",
            "addi a0,a0,1",
        )]
        self.assertEqual(
            _symbolic_jal_indices(lines), [0, 1, 2, 3, 4]
        )

    def test_directions(self):
        for text, expected in (
            ("jal ra, 20f", "f"),
            ("j 21b", "b"),
            ("jal 22f", "f"),
            ("jal zero, 23b", "b"),
            ("jal ra, 20", None),
            ("j zero", None),
        ):
            with self.subTest(text=text):
                self.assertEqual(
                    _symbolic_jump_direction(text), expected
                )

    def test_backward_insertion(self):
        lines = [("20:", None), ("j 20b", None)]
        out = _insert_jal_nops(lines, [1])

        self.assertEqual(out[0], lines[0])
        self.assertEqual(out[-1], lines[1])
        self.assertEqual(out[-3][0], "c.nop")

    def test_forward_insertion(self):
        lines = [("jal ra, 20f", None), ("20:", None)]
        out = _insert_jal_nops(lines, [0])

        self.assertEqual(out[0], lines[0])
        self.assertEqual(out[-1], lines[1])
        self.assertEqual(out[3][0], "c.nop")

    def test_invalid_index_rejected(self):
        with self.assertRaises(ValueError):
            _insert_jal_nops(
                [("addi a0,a0,1", None)], [0]
            )

    def test_linker_marker(self):
        source = [("20:", None), ("j 20b", None)]
        asm = format_asm_output(
            "test", source, [], {}, (1,)
        )

        self.assertIn(".word .Lbio_jal_1", asm)
        self.assertLess(
            asm.index(".Lbio_jal_1:"),
            asm.index("    j 20b"),
        )


if __name__ == "__main__":
    unittest.main()
