#!/usr/bin/env python3
"""
rs_errata_check.py - Check and patch RISC-V assembly errata in Rust .rs files.

Scans Rust source files for bio_code! and bio_code_aligned! macros, extracts
the inline assembly strings, checks each instruction for known BIO coprocessor
errata, and optionally patches the file in-place.

Usage:
    python3 rs_errata_check.py <file.rs> [file2.rs ...]   # check only (default)
    python3 rs_errata_check.py --autopatch <file.rs> ...   # patch in-place

Errata covered:
  BUG 1 ("phantom rs1"): For lui/auipc/jal, bits [19:15] of the instruction
    encoding are not gated. If that field = 16-19, a spurious FIFO read occurs.
  BUG 2 ("phantom rs2"): For non-R/S/B-type instructions, bits [24:20] = 20
    triggers a spurious quantum signal.

Exit codes:
    0 - No errata found (or all patched with --autopatch)
    1 - Errata found (check mode)
    2 - Usage / file error
"""

import argparse
import re
import sys
import os

# ============================================================================
# Constants
# ============================================================================

# Temp register for errata workarounds. Saved/restored via stack.
ERRATA_TEMP = "t0"
ERRATA_STACK_OFFSET = -4  # bits[4:0] = 0b11100, safe (not 20)

# Bug 1: register numbers that trigger phantom rs1 reads
BUG1_BAD_REGS = {16, 17, 18, 19}

# Register ABI name mapping (for display/normalization)
ABI_TO_X = {
    "zero": "x0", "ra": "x1", "sp": "x2", "gp": "x3",
    "tp": "x4", "t0": "x5", "t1": "x6", "t2": "x7",
    "s0": "x8", "fp": "x8", "s1": "x9",
    "a0": "x10", "a1": "x11", "a2": "x12", "a3": "x13",
    "a4": "x14", "a5": "x15", "a6": "x16", "a7": "x17",
    "s2": "x18", "s3": "x19", "s4": "x20", "s5": "x21",
    "s6": "x22", "s7": "x23", "s8": "x24", "s9": "x25",
    "s10": "x26", "s11": "x27",
    "t3": "x28", "t4": "x29", "t5": "x30", "t6": "x31",
}


# ============================================================================
# Instruction parsing helpers (adapted from clang2rustasm.py errata system)
# ============================================================================

def _parse_instruction(instr_str):
    """
    Parse an instruction string into (mnemonic, [operands]).
    Handles both regular "add a0, a1, a2" and memory "lw a0, 4(sp)" forms.
    Returns (mnemonic, operands_list) or (None, []) if not parseable.
    """
    if instr_str is None:
        return None, []
    s = instr_str.strip()
    # Labels (e.g. "20:") are not instructions
    if re.match(r'^\d+\s*:', s):
        return None, []
    # Bare label like "20: lw x8, 0(x8)" - extract instruction after colon
    m = re.match(r'^\d+\s*:\s*(.*)', s)
    if m:
        s = m.group(1).strip()
        if not s:
            return None, []
    # Directives
    if s.startswith('.'):
        return None, []
    parts = s.split(None, 1)
    if not parts:
        return None, []
    mnemonic = parts[0].lower()
    if len(parts) == 1:
        return mnemonic, []
    operands = [op.strip() for op in parts[1].split(',')]
    return mnemonic, operands


def _parse_mem_operand(operand):
    """
    Parse a memory operand like "4(sp)" or "-8(x2)".
    Returns (offset_int, base_reg_str) or (None, None) if not a memory operand.
    """
    m = re.match(r'^(-?\d+)\((\w+)\)$', operand.strip())
    if m:
        return int(m.group(1)), m.group(2)
    return None, None


def _imm_to_int(s):
    """Parse an immediate string (decimal or 0x hex) to int. Returns None on failure."""
    s = s.strip()
    try:
        return int(s, 0)
    except (ValueError, TypeError):
        return None


def _has_relocation(instr_str):
    """Check if instruction contains a relocation like %hi(...) or %lo(...)."""
    return '%' in instr_str if instr_str else False


def _is_label_ref(s):
    """Check if a string looks like a label reference (e.g. '20f', '30b')."""
    return bool(re.match(r'^\d+[fb]$', s.strip()))


# ============================================================================
# Bug detection functions
# ============================================================================

def _u_type_phantom_rs1(imm20):
    """U-type (lui/auipc): instruction bits [19:15] = imm[7:3]."""
    field = (imm20 >> 3) & 0x1F
    return field in BUG1_BAD_REGS


def _u_type_phantom_rs2(imm20):
    """Bug 2 for U-type: instruction bits [24:20] = imm[12:8]."""
    field = (imm20 >> 8) & 0x1F
    return field == 20


def _j_type_phantom_rs1(imm21):
    """J-type (jal): instruction bits [19:15] = imm[19:15]."""
    field = (imm21 >> 15) & 0x1F
    return field in BUG1_BAD_REGS


def _j_type_phantom_rs2(imm21):
    """
    Bug 2 for J-type (jal): bits [24:20].
    J-type: inst[30:21] = imm[10:1], inst[20] = imm[11]
    So inst[24:20] = {imm[4:1], imm[11]}
    field = ((imm >> 1) & 0xF) << 1 | ((imm >> 11) & 1)
    """
    field = (((imm21 >> 1) & 0xF) << 1) | ((imm21 >> 11) & 1)
    return field == 20


def _i_type_imm_has_bug2(imm12):
    """For I-type, bits [24:20] = imm[4:0]. Bug when lower 5 bits = 20."""
    encoded = imm12 & 0xFFF
    return (encoded & 0x1F) == 20


# ============================================================================
# Patch generation helpers
# ============================================================================

def _save_temp():
    return f"sw {ERRATA_TEMP}, {ERRATA_STACK_OFFSET}(sp)"

def _restore_temp():
    return f"lw {ERRATA_TEMP}, {ERRATA_STACK_OFFSET}(sp)"


def _build_constant_in_reg(temp_reg, diff):
    """
    Build instructions to load `diff` into temp_reg.
    Careful not to trigger errata in generated code.
    Returns list of instruction strings.
    """
    instrs = []
    if diff == 0:
        return []

    lowest_bit = (diff & -diff).bit_length() - 1
    shifted_val = diff >> lowest_bit

    if shifted_val < 2048:
        if _i_type_imm_has_bug2(shifted_val):
            instrs.append(f"addi {temp_reg}, zero, {shifted_val - 1}")
            instrs.append(f"addi {temp_reg}, {temp_reg}, 1")
        else:
            instrs.append(f"addi {temp_reg}, zero, {shifted_val}")

        remaining_shift = lowest_bit
        while remaining_shift > 0:
            if remaining_shift >= 20:
                instrs.append(f"slli {temp_reg}, {temp_reg}, 19")
                remaining_shift -= 19
            elif _i_type_imm_has_bug2(remaining_shift):
                instrs.append(f"slli {temp_reg}, {temp_reg}, {remaining_shift - 1}")
                instrs.append(f"slli {temp_reg}, {temp_reg}, 1")
                remaining_shift = 0
            else:
                instrs.append(f"slli {temp_reg}, {temp_reg}, {remaining_shift}")
                remaining_shift = 0
    else:
        if shifted_val < (1 << 20):
            upper = shifted_val >> 12
            lower = shifted_val & 0xFFF
            if lower >= 0x800:
                upper += 1
                lower = lower - 0x1000
            if not _u_type_phantom_rs1(upper) and not _u_type_phantom_rs2(upper):
                instrs.append(f"lui {temp_reg}, {upper}")
                if lower != 0:
                    if _i_type_imm_has_bug2(lower):
                        instrs.append(f"addi {temp_reg}, {temp_reg}, {lower - 1}")
                        instrs.append(f"addi {temp_reg}, {temp_reg}, 1")
                    else:
                        instrs.append(f"addi {temp_reg}, {temp_reg}, {lower}")
            else:
                instrs.append(f"addi {temp_reg}, zero, {(shifted_val >> 10) & 0x7FF}")
                instrs.append(f"slli {temp_reg}, {temp_reg}, 10")
                rest = shifted_val & 0x3FF
                if rest:
                    instrs.append(f"addi {temp_reg}, {temp_reg}, {rest}")

            remaining_shift = lowest_bit
            while remaining_shift > 0:
                chunk = min(remaining_shift, 19) if remaining_shift >= 20 else remaining_shift
                if _i_type_imm_has_bug2(chunk):
                    instrs.append(f"slli {temp_reg}, {temp_reg}, {chunk - 1}")
                    instrs.append(f"slli {temp_reg}, {temp_reg}, 1")
                else:
                    instrs.append(f"slli {temp_reg}, {temp_reg}, {chunk}")
                remaining_shift -= chunk

    return instrs


# ============================================================================
# Instruction-specific checkers/patchers
#
# Each function returns:
#   None              - instruction is safe, no patch needed
#   (bug_desc, [patched_instructions])  - errata found; patched_instructions
#     is a list of replacement instruction strings
# ============================================================================

def _check_lui(mnemonic, operands, original_instr):
    """Check/patch lui rd, imm20 for Bug 1 and Bug 2."""
    if len(operands) != 2:
        return None
    rd = operands[0]
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None

    imm20 = imm & 0xFFFFF
    has_bug1 = _u_type_phantom_rs1(imm20)
    has_bug2 = _u_type_phantom_rs2(imm20)

    if not has_bug1 and not has_bug2:
        return None

    bugs = []
    if has_bug1:
        bugs.append("B1:phantom-rs1")
    if has_bug2:
        bugs.append("B2:bits24:20=20")
    bug_desc = "+".join(bugs)

    safe_imm20 = imm20
    if has_bug1:
        safe_imm20 = safe_imm20 & ~(0x1F << 3)
    if has_bug2:
        safe_imm20 = safe_imm20 & ~(0x1F << 8)

    diff = (imm20 - safe_imm20) & 0xFFFFF
    fixup_instrs = _build_constant_in_reg(ERRATA_TEMP, diff)

    patch = []
    if safe_imm20 != 0:
        patch.append(f"lui {rd}, 0x{safe_imm20:x}")
    else:
        patch.append(f"addi {rd}, zero, 0")

    if diff != 0:
        patch.append(_save_temp())
        patch.extend(fixup_instrs)
        patch.append(f"slli {ERRATA_TEMP}, {ERRATA_TEMP}, 12")
        patch.append(f"add {rd}, {rd}, {ERRATA_TEMP}")
        patch.append(_restore_temp())

    return (bug_desc, patch)


def _check_auipc(mnemonic, operands, original_instr):
    """Check/patch auipc rd, imm20 for Bug 1 and Bug 2."""
    if len(operands) != 2:
        return None
    rd = operands[0]
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None

    imm20 = imm & 0xFFFFF
    has_bug1 = _u_type_phantom_rs1(imm20)
    has_bug2 = _u_type_phantom_rs2(imm20)

    if not has_bug1 and not has_bug2:
        return None

    bugs = []
    if has_bug1:
        bugs.append("B1:phantom-rs1")
    if has_bug2:
        bugs.append("B2:bits24:20=20")
    bug_desc = "+".join(bugs)

    safe_imm20 = imm20
    if has_bug1:
        safe_imm20 = safe_imm20 & ~(0x1F << 3)
    if has_bug2:
        safe_imm20 = safe_imm20 & ~(0x1F << 8)

    diff = (imm20 - safe_imm20) & 0xFFFFF

    patch = []
    patch.append(f"auipc {rd}, 0x{safe_imm20:x}")
    if diff != 0:
        patch.append(_save_temp())
        fixup_instrs = _build_constant_in_reg(ERRATA_TEMP, diff)
        patch.extend(fixup_instrs)
        patch.append(f"slli {ERRATA_TEMP}, {ERRATA_TEMP}, 12")
        patch.append(f"add {rd}, {rd}, {ERRATA_TEMP}")
        patch.append(_restore_temp())

    return (bug_desc, patch)


def _check_jal(mnemonic, operands, original_instr):
    """Check jal for Bug 1 and Bug 2 (warns only - numeric jal is rare)."""
    if len(operands) == 1:
        return None  # pseudo form with label
    if len(operands) != 2:
        return None
    rd = operands[0]
    if _is_label_ref(operands[1]):
        return None  # label reference, can't analyze statically
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None

    imm21 = imm & 0x1FFFFF
    has_bug1 = _j_type_phantom_rs1(imm21)
    has_bug2 = _j_type_phantom_rs2(imm21)

    if not has_bug1 and not has_bug2:
        return None

    bugs = []
    if has_bug1:
        bugs.append("B1:phantom-rs1")
    if has_bug2:
        bugs.append("B2:bits24:20=20")
    bug_desc = "+".join(bugs) + " WARNING:manual-review-needed"

    # Can't auto-patch jal reliably, keep original with warning
    return (bug_desc, [original_instr])


def _check_shift_imm(mnemonic, operands, original_instr):
    """Check/patch slli/srli/srai where shamt == 20."""
    if len(operands) != 3:
        return None
    rd, rs1, shamt_str = operands
    shamt = _imm_to_int(shamt_str)
    if shamt is None:
        return None
    if (shamt & 0x1F) != 20:
        return None

    patch = [
        f"{mnemonic} {rd}, {rs1}, {shamt - 1}",
        f"{mnemonic} {rd}, {rd}, 1",
    ]
    return (f"B2:shamt={shamt}", patch)


def _check_load(mnemonic, operands, original_instr):
    """Check/patch load instructions where offset triggers Bug 2."""
    if len(operands) != 2:
        return None
    rd = operands[0]
    offset, base = _parse_mem_operand(operands[1])
    if offset is None:
        return None

    offset_enc = offset & 0xFFF
    if (offset_enc & 0x1F) != 20:
        return None

    # Find a safe adjustment delta
    delta = None
    new_offset = None
    for adj in [-4, 4, -8, 8, -12, 12, -16, 16]:
        candidate = offset - adj
        candidate_enc = candidate & 0xFFF
        if (candidate_enc & 0x1F) != 20:
            adj_enc = adj & 0xFFF
            neg_adj_enc = (-adj) & 0xFFF
            if (adj_enc & 0x1F) != 20 and (neg_adj_enc & 0x1F) != 20:
                if -2048 <= candidate <= 2047:
                    delta = adj
                    new_offset = candidate
                    break

    if delta is None:
        delta = -4
        new_offset = offset + 4

    patch = []
    if rd == base:
        patch.append(f"addi {ERRATA_TEMP}, {base}, {delta}")
        patch.append(f"{mnemonic} {rd}, {new_offset}({ERRATA_TEMP})")
    else:
        patch.append(f"addi {base}, {base}, {delta}")
        patch.append(f"{mnemonic} {rd}, {new_offset}({base})")
        patch.append(f"addi {base}, {base}, {-delta}")

    return (f"B2:offset={offset},bits[4:0]={offset_enc & 0x1F}", patch)


def _check_store(mnemonic, operands, original_instr):
    """Stores are S-type: bits[24:20] = rs2 register, hardware handles correctly."""
    return None


def _check_i_type_alu(mnemonic, operands, original_instr):
    """Check/patch addi/andi/ori/xori/slti/sltiu where imm triggers Bug 2."""
    if len(operands) != 3:
        return None
    rd, rs1, imm_str = operands
    imm = _imm_to_int(imm_str)
    if imm is None:
        return None

    imm_enc = imm & 0xFFF
    if (imm_enc & 0x1F) != 20:
        return None

    patch = []

    if mnemonic == 'addi':
        new_imm = imm - 1
        new_enc = new_imm & 0xFFF
        if (new_enc & 0x1F) == 20:
            new_imm = imm + 1
            patch.append(f"addi {rd}, {rs1}, {new_imm}")
            patch.append(f"addi {rd}, {rd}, -1")
        else:
            patch.append(f"addi {rd}, {rs1}, {new_imm}")
            patch.append(f"addi {rd}, {rd}, 1")
    elif mnemonic in ('andi', 'ori', 'xori'):
        r_type_op = {'andi': 'and', 'ori': 'or', 'xori': 'xor'}[mnemonic]
        patch.append(_save_temp())
        if (imm_enc & 0x1F) == 20:
            patch.append(f"addi {ERRATA_TEMP}, zero, {imm - 1}")
            patch.append(f"addi {ERRATA_TEMP}, {ERRATA_TEMP}, 1")
        else:
            patch.append(f"addi {ERRATA_TEMP}, zero, {imm}")
        patch.append(f"{r_type_op} {rd}, {rs1}, {ERRATA_TEMP}")
        patch.append(_restore_temp())
    elif mnemonic in ('slti', 'sltiu'):
        r_type_op = {'slti': 'slt', 'sltiu': 'sltu'}[mnemonic]
        patch.append(_save_temp())
        patch.append(f"addi {ERRATA_TEMP}, zero, {imm - 1}")
        patch.append(f"addi {ERRATA_TEMP}, {ERRATA_TEMP}, 1")
        patch.append(f"{r_type_op} {rd}, {rs1}, {ERRATA_TEMP}")
        patch.append(_restore_temp())
    else:
        return None

    return (f"B2:imm={imm},bits[4:0]={imm_enc & 0x1F}", patch)


def _check_jalr(mnemonic, operands, original_instr):
    """Check/patch jalr where offset triggers Bug 2."""
    if len(operands) != 2:
        return None
    rd = operands[0]
    offset, rs1 = _parse_mem_operand(operands[1])
    if offset is None:
        return None

    offset_enc = offset & 0xFFF
    if (offset_enc & 0x1F) != 20:
        return None

    delta = -4
    new_offset = offset + 4
    new_offset_enc = new_offset & 0xFFF
    if (new_offset_enc & 0x1F) == 20:
        delta = -8
        new_offset = offset + 8

    patch = []
    if rd == rs1:
        patch.append(_save_temp())
        patch.append(f"addi {ERRATA_TEMP}, {rs1}, {delta}")
        patch.append(f"jalr {rd}, {new_offset}({ERRATA_TEMP})")
        patch.append(_restore_temp())
    else:
        patch.append(f"addi {rs1}, {rs1}, {delta}")
        patch.append(f"jalr {rd}, {new_offset}({rs1})")
        patch.append(f"addi {rs1}, {rs1}, {-delta}")

    return (f"B2:offset={offset}", patch)


# Fence/system instructions: safe
def _check_fence_or_system(mnemonic, operands, original_instr):
    return None


# ============================================================================
# Dispatch table
# ============================================================================

ERRATA_DISPATCH = {
    'lui':    _check_lui,
    'auipc':  _check_auipc,
    'jal':    _check_jal,
    'slli':   _check_shift_imm,
    'srli':   _check_shift_imm,
    'srai':   _check_shift_imm,
    'lw':     _check_load,
    'lh':     _check_load,
    'lb':     _check_load,
    'lhu':    _check_load,
    'lbu':    _check_load,
    'addi':   _check_i_type_alu,
    'andi':   _check_i_type_alu,
    'ori':    _check_i_type_alu,
    'xori':   _check_i_type_alu,
    'slti':   _check_i_type_alu,
    'sltiu':  _check_i_type_alu,
    'jalr':   _check_jalr,
    'sw':     _check_store,
    'sh':     _check_store,
    'sb':     _check_store,
    'fence':  _check_fence_or_system,
    'ecall':  _check_fence_or_system,
    'ebreak': _check_fence_or_system,
}


# ============================================================================
# Rust file parsing: extract assembly from bio_code! macros
# ============================================================================

class MacroSpan:
    """Represents the location of a bio_code! or bio_code_aligned! macro in the file."""
    def __init__(self, macro_name, fn_name, start_line, end_line):
        self.macro_name = macro_name  # "bio_code" or "bio_code_aligned"
        self.fn_name = fn_name        # first argument to the macro
        self.start_line = start_line  # 0-indexed line number of macro opening
        self.end_line = end_line      # 0-indexed line number of closing ");"


class AsmLine:
    """An assembly instruction extracted from a quoted string inside a bio_code! macro."""
    def __init__(self, file_line_idx, instr_text, leading_whitespace, quote_char,
                 prefix_text, suffix_text, has_trailing_comma, full_line):
        self.file_line_idx = file_line_idx      # 0-indexed line in the file
        self.instr_text = instr_text            # the assembly text inside the quotes
        self.leading_whitespace = leading_whitespace
        self.quote_char = quote_char            # " or '
        self.prefix_text = prefix_text          # text before the opening quote on this line
        self.suffix_text = suffix_text          # text after the closing quote (includes comma, comment)
        self.has_trailing_comma = has_trailing_comma
        self.full_line = full_line              # the complete original line


def find_macro_spans(lines):
    """
    Find all bio_code! and bio_code_aligned! macro invocations in the file.
    Returns list of MacroSpan.
    """
    spans = []
    i = 0
    # Match the macro invocation line:
    #   bio_code!(fn_name, START, END,
    #   bio_code_aligned!(fn_name, START, END,
    # The macro may also use square brackets: bio_code![...] but parens are standard.
    macro_pat = re.compile(
        r'^(\s*)(?:#\[.*\]\s*)?'  # optional attributes like #[rustfmt::skip]
        r'(bio_code(?:_aligned)?)\s*!\s*\(\s*'
        r'(\w+)'  # fn_name
    )

    while i < len(lines):
        m = macro_pat.match(lines[i])
        if m:
            macro_name = m.group(2)
            fn_name = m.group(3)
            start_line = i
            # Find the closing ");", handling multi-line macros
            depth = 0
            j = i
            found_end = False
            while j < len(lines):
                line = lines[j]
                for ch in line:
                    if ch == '(':
                        depth += 1
                    elif ch == ')':
                        depth -= 1
                        if depth == 0:
                            found_end = True
                            break
                if found_end:
                    spans.append(MacroSpan(macro_name, fn_name, start_line, j))
                    i = j + 1
                    break
                j += 1
            else:
                # Never found closing paren
                i += 1
        else:
            i += 1

    return spans


def extract_asm_lines(lines, span):
    """
    Extract AsmLine objects from a macro span.

    Each assembly instruction is a quoted string on its own line:
        "mv x4, x17",    // comment
        "li x9, 0x1000000",

    We need to identify lines containing quoted assembly strings (as opposed to
    the macro header line, label-only lines, directive lines, etc.).
    """
    asm_lines = []

    # The first line is the macro invocation header; skip it.
    # Assembly strings start from the line after the header (or the header itself
    # if the first string is on the same line, but typically they're on separate lines).
    # We also need to handle the macro args (fn_name, START_LABEL, END_LABEL) which
    # may span the first line or first few lines.

    # Strategy: scan each line in the span for quoted strings that look like assembly.
    # A line with assembly looks like:
    #     "instruction here",     // optional comment
    #     "20: instruction",
    #     "20:",                  // label-only lines
    #     ".p2align 2",
    # We skip the macro opening line's arguments.

    # Find where the header arguments end (after the third comma following the macro name)
    header_end = _find_header_end(lines, span)

    for idx in range(header_end, span.end_line + 1):
        line = lines[idx]
        asm = _extract_asm_from_line(line, idx)
        if asm is not None:
            asm_lines.append(asm)

    return asm_lines


def _find_header_end(lines, span):
    """
    Find the line index where the macro header arguments end.
    The header is: macro_name!(fn_name, START_LABEL, END_LABEL,
    After the third comma, the assembly strings begin.
    Returns the line index where assembly strings start.
    """
    comma_count = 0
    for idx in range(span.start_line, span.end_line + 1):
        line = lines[idx]
        in_string = False
        for ch in line:
            if ch == '"' and not in_string:
                in_string = True
            elif ch == '"' and in_string:
                in_string = False
            elif ch == ',' and not in_string:
                comma_count += 1
                if comma_count >= 3:
                    # The assembly starts on the next line (or the rest of this line)
                    return idx + 1
    # Fallback: start from line after the macro opening
    return span.start_line + 1


def _extract_asm_from_line(line, line_idx):
    """
    Extract assembly text from a single line containing a quoted string.
    Returns AsmLine or None.
    """
    # Match: optional whitespace, opening quote, content, closing quote, optional comma, optional comment
    m = re.match(
        r'^(\s*)'           # leading whitespace
        r'(.*?)'            # prefix before quote (e.g. nothing, or part of multi-string line)
        r'"([^"]*)"'        # quoted string
        r'(\s*,?)'          # optional trailing comma
        r'(\s*(?://.*)?)'   # optional comment
        r'\s*$',
        line
    )
    if not m:
        return None

    leading_ws = m.group(1)
    prefix = m.group(2)
    instr_text = m.group(3)
    comma_part = m.group(4)
    comment_part = m.group(5)

    has_comma = ',' in comma_part
    suffix = comma_part + comment_part

    return AsmLine(
        file_line_idx=line_idx,
        instr_text=instr_text,
        leading_whitespace=leading_ws,
        quote_char='"',
        prefix_text=prefix,
        suffix_text=suffix,
        has_trailing_comma=has_comma,
        full_line=line,
    )


# ============================================================================
# Check a single instruction for errata
# ============================================================================

class ErrataFinding:
    """One errata finding on a specific instruction."""
    def __init__(self, asm_line, bug_desc, patched_instrs):
        self.asm_line = asm_line            # AsmLine object
        self.bug_desc = bug_desc            # human-readable bug description
        self.patched_instrs = patched_instrs  # list of replacement instruction strings


def check_instruction(asm_line):
    """
    Check a single AsmLine for errata.
    Returns ErrataFinding or None.
    """
    instr_text = asm_line.instr_text.strip()

    # Skip empty, labels-only, directives, comments
    if not instr_text:
        return None
    # Skip lines that are only comments inside the string
    if instr_text.startswith('//'):
        return None
    # Skip relocation references
    if _has_relocation(instr_text):
        return None

    # Handle "label: instruction" form - check the instruction part
    label_prefix = ""
    lm = re.match(r'^(\d+\s*:\s*)(.*)', instr_text)
    if lm:
        label_prefix = lm.group(1)
        instr_text = lm.group(2).strip()
        if not instr_text:
            return None  # label-only

    # Strip inline assembly comments (after //)
    # Actually in the Rust bio_code! format, comments are outside the quotes.
    # But some inline comments may be inside with "// comment" style.
    # Let's be safe:
    comment_pos = instr_text.find('//')
    if comment_pos >= 0:
        instr_text = instr_text[:comment_pos].strip()
    if not instr_text:
        return None

    mnemonic, operands = _parse_instruction(instr_text)
    if mnemonic is None:
        return None

    check_fn = ERRATA_DISPATCH.get(mnemonic)
    if check_fn is None:
        return None  # not in dispatch table

    result = check_fn(mnemonic, operands, instr_text)
    if result is None:
        return None

    bug_desc, patched_instrs = result

    # If there was a label prefix, prepend it to the first patched instruction
    if label_prefix and patched_instrs:
        patched_instrs = [label_prefix + patched_instrs[0]] + patched_instrs[1:]

    return ErrataFinding(asm_line, bug_desc, patched_instrs)


# ============================================================================
# File-level checking
# ============================================================================

def check_file(filepath):
    """
    Check a .rs file for errata.
    Returns (lines, list_of_findings, list_of_macro_spans).
    """
    with open(filepath, 'r') as f:
        lines = f.readlines()

    # Strip trailing newlines but remember them
    raw_lines = [line.rstrip('\n').rstrip('\r') for line in lines]

    spans = find_macro_spans(raw_lines)
    findings = []

    for span in spans:
        asm_lines = extract_asm_lines(raw_lines, span)
        for asm_line in asm_lines:
            finding = check_instruction(asm_line)
            if finding is not None:
                findings.append(finding)

    return raw_lines, findings, spans


# ============================================================================
# Reporting (check mode)
# ============================================================================

def report_findings(filepath, findings):
    """Print a human-readable report of errata findings."""
    if not findings:
        print(f"{filepath}: OK (no errata found)")
        return

    print(f"{filepath}: {len(findings)} errata finding(s)")
    for f in findings:
        line_num = f.asm_line.file_line_idx + 1  # 1-indexed for display
        instr = f.asm_line.instr_text.strip()
        print(f"  line {line_num}: {f.bug_desc}")
        print(f"    original:  {instr}")
        if f.patched_instrs:
            print(f"    patched ({len(f.patched_instrs)} instructions):")
            for pi in f.patched_instrs:
                print(f"      {pi}")
        print()


# ============================================================================
# Patching (--autopatch mode)
# ============================================================================

def apply_patches(raw_lines, findings):
    """
    Apply all patches to the file lines.
    Returns new list of lines.

    Strategy: process findings in reverse line order so that inserting/replacing
    lines doesn't shift indices of earlier findings.
    """
    # Group findings by line index. There should be at most one finding per line.
    by_line = {}
    for f in findings:
        idx = f.asm_line.file_line_idx
        if idx in by_line:
            # Multiple findings on same line shouldn't happen, take the first
            pass
        else:
            by_line[idx] = f

    result = list(raw_lines)

    # Process in reverse order
    for line_idx in sorted(by_line.keys(), reverse=True):
        finding = by_line[line_idx]
        asm = finding.asm_line
        patched = finding.patched_instrs

        if not patched:
            continue

        # Build replacement lines
        ws = asm.leading_whitespace
        prefix = asm.prefix_text

        replacement = []

        # Add errata comment
        replacement.append(f"{ws}// ERRATA {finding.bug_desc} patch for: {asm.instr_text.strip()}")

        for i, pinstr in enumerate(patched):
            is_last = (i == len(patched) - 1)
            # The last patched instruction gets the original line's trailing comma and comment
            if is_last:
                comma = "," if asm.has_trailing_comma else ""
                # Preserve the original comment if any
                comment_match = re.search(r'(//.*)', asm.suffix_text)
                comment = f" {comment_match.group(1)}" if comment_match else ""
                replacement.append(f'{ws}{prefix}"{pinstr}"{comma}{comment}')
            else:
                replacement.append(f'{ws}{prefix}"{pinstr}",')

        result[line_idx:line_idx + 1] = replacement

    return result


def patch_file(filepath, raw_lines, findings):
    """Write patched file back to disk."""
    patched_lines = apply_patches(raw_lines, findings)

    with open(filepath, 'w') as f:
        for line in patched_lines:
            f.write(line + '\n')

    return len(findings)


# ============================================================================
# Main
# ============================================================================

def parse_args():
    parser = argparse.ArgumentParser(
        description="Check and patch RISC-V assembly errata in Rust .rs files"
    )
    parser.add_argument(
        "files",
        nargs="+",
        help=".rs files to check",
    )
    parser.add_argument(
        "--autopatch",
        action="store_true",
        help="Automatically patch errata in-place (replaces files)",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Show detailed output even when no errata found",
    )
    return parser.parse_args()


def main():
    args = parse_args()

    total_findings = 0
    total_files_with_findings = 0
    total_patched = 0

    for filepath in args.files:
        if not os.path.exists(filepath):
            print(f"Error: file not found: {filepath}", file=sys.stderr)
            sys.exit(2)

        raw_lines, findings, spans = check_file(filepath)

        if args.verbose or findings:
            if not findings:
                macro_names = [s.fn_name for s in spans]
                print(f"{filepath}: OK ({len(spans)} macro(s) scanned: {', '.join(macro_names)})")
            else:
                total_files_with_findings += 1

        total_findings += len(findings)

        if findings:
            if args.autopatch:
                count = patch_file(filepath, raw_lines, findings)
                total_patched += count
                print(f"{filepath}: patched {count} errata finding(s) in-place")
            else:
                report_findings(filepath, findings)

    # Summary
    if len(args.files) > 1 or args.verbose:
        print(f"\nSummary: {len(args.files)} file(s) scanned, "
              f"{total_findings} finding(s) in {total_files_with_findings} file(s)")
        if args.autopatch:
            print(f"  {total_patched} patches applied")

    if total_findings > 0 and not args.autopatch:
        sys.exit(1)
    sys.exit(0)


if __name__ == "__main__":
    main()