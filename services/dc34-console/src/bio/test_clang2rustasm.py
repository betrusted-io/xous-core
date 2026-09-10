from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import clang2rustasm


class ExtractAllCodeTests(unittest.TestCase):
    @staticmethod
    def sample_assembly():
        return [
            '.section .text.first,"ax",@progbits\n',
            "first:\n",
            "    .cfi_startproc\n",
            "    .cfi_def_cfa_offset 16\n",
            "    addi a0, a0, 1\n",
            ".Lfunc_end0:\n",
            "    .size first, .Lfunc_end0-first\n",
            "    .cfi_endproc\n",
            '.section .text.second,"ax",@progbits\n',
            "second:\n",
            "    .cfi_startproc\n",
            "    ret\n",
            ".Lfunc_end1:\n",
            "    .cfi_endproc\n",
        ]

    def test_strips_cfi_directives_from_multiple_functions(self):
        extracted = clang2rustasm.extract_all_code(self.sample_assembly())

        self.assertIn("first:", extracted)
        self.assertIn("addi a0, a0, 1", extracted)
        self.assertIn("second:", extracted)
        self.assertIn("ret", extracted)
        self.assertFalse(any(line.startswith(".cfi") for line in extracted))

    def test_cli_output_contains_no_cfi_directives(self):
        script = Path(__file__).with_name("clang2rustasm.py")
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir)
            zig_out = temp_path / "zig-out"
            zig_out.mkdir()
            (zig_out / "sample.s").write_text("".join(self.sample_assembly()))
            output = temp_path / "sample.rs"

            result = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "sample",
                    "--zig-out",
                    str(zig_out),
                    "--output",
                    str(output),
                ],
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn(".cfi", output.read_text())

    def test_missing_input_recommends_existing_asm_only_build(self):
        script = Path(__file__).with_name("clang2rustasm.py")
        with tempfile.TemporaryDirectory() as temp_dir:
            result = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "missing",
                    "--zig-out",
                    temp_dir,
                ],
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("-Dasm-only=true", result.stderr)
            self.assertNotIn("build dis", result.stderr)


if __name__ == "__main__":
    unittest.main()
