#!/usr/bin/env python3
"""
clang2rustasm.py - Convert clang RISC-V assembly output to Rust bio_code! macro invocations.

Usage:
    python3 clang2rustasm.py <module>  [options]

  <module> is a subdirectory name such as "math_test".
  The script looks for zig-out/<module>.s (produced by "zig build dis -Dmodule=<module>")
  and writes the converted Rust source to <module>/<module>.rs.

Options:
    --zig-out DIR     Override the zig-out search directory (default: zig-out)
    --fn-name NAME    Function name for bio_code! (default: derived from module)
    --start-label L   Start label name (default: derived from module)
    --end-label L     End label name (default: derived from module)
    --label-base N    Starting numeric label number (default: 20)
    -o FILE           Override output file path
"""

import argparse
import re
import sys
import os
from datetime import datetime

# ============================================================================
# Register translation: x16-x31 ABI names -> raw xN form
# x0-x15 ABI names are left as-is
# ============================================================================
HIGH_REG_MAP = {
    "a6": "x16",
    "a7": "x17",
    "s2": "x18",
    "s3": "x19",
    "s4": "x20",
    "s5": "x21",
    "s6": "x22",
    "s7": "x23",
    "s8": "x24",
    "s9": "x25",
    "s10": "x26",
    "s11": "x27",
    "t3": "x28",
    "t4": "x29",
    "t5": "x30",
    "t6": "x31",
}

# Directives to strip entirely
STRIP_DIRECTIVES = {
    ".attribute",
    ".file",
    ".ident",
    ".addrsig",
}

# Section directives to strip (matched by substring)
STRIP_SECTION_NAMES = {
    '".note.GNU-stack"',
}

# Data directives we know how to handle, mapped to their byte width
DATA_DIRECTIVE_WIDTH = {
    ".byte":  1,
    ".half":  2,
    ".short": 2,
    ".word":  4,
    ".long":  4,
}

WARNINGS = []

# ============================================================================
# Argument parsing and path resolution
# ============================================================================

def parse_args():
    parser = argparse.ArgumentParser(
        description="Convert clang RISC-V assembly to Rust bio_code! macro"
    )
    parser.add_argument(
        "module",
        help=(
            "Module name (subdirectory). "
            "Reads zig-out/<module>.s and writes <module>/<module>.rs."
        ),
    )
    parser.add_argument(
        "-o", "--output",
        help="Override output .rs file path",
    )
    parser.add_argument(
        "--zig-out",
        default="zig-out",
        metavar="DIR",
        help="Directory where zig places compiled .s files (default: zig-out)",
    )
    parser.add_argument("--fn-name",     help="Function name for bio_code! (default: derived from module)")
    parser.add_argument("--start-label", help="Start label name (default: derived from module)")
    parser.add_argument("--end-label",   help="End label name (default: derived from module)")
    parser.add_argument(
        "--label-base",
        type=int,
        default=20,
        help="Starting numeric label number (default: 20)",
    )
    return parser.parse_args()


def resolve_paths(args):
    """
    Resolve the input .s path and output .rs path from the module name.

    Input:  <zig-out>/<module>.s
    Output: <module>/<module>.rs   (or -o override)
    """
    module = args.module
    module = module.rstrip("/\\")
    if module.endswith(".s"):
        module = module[:-2]

    safe = re.sub(r"[^a-zA-Z0-9]", "_", module).strip("_").lower()

    fn_name     = args.fn_name     or f"{safe}_bio_code"
    start_label = args.start_label or f"BM_{safe.upper()}_BIO_START"
    end_label   = args.end_label   or f"BM_{safe.upper()}_BIO_END"

    input_path = os.path.join(args.zig_out, f"{module}.s")

    if args.output:
        output_path = args.output
    else:
        output_path = os.path.join(module, f"{module}.rs")

    return input_path, output_path, fn_name, start_label, end_label


# ============================================================================
# Directive helpers
# ============================================================================

def should_strip_directive(stripped):
    for directive in STRIP_DIRECTIVES:
        if stripped.startswith(directive):
            return True
    return False


def is_strip_section(stripped):
    if stripped.startswith(".section"):
        for name in STRIP_SECTION_NAMES:
            if name in stripped:
                return True
    return False


def replace_registers(text):
    """Replace ABI register names for x16-x31 with raw xN form."""
    for abi_name in sorted(HIGH_REG_MAP.keys(), key=len, reverse=True):
        raw_name = HIGH_REG_MAP[abi_name]
        pattern = r"(?<![a-zA-Z0-9_])" + re.escape(abi_name) + r"(?![a-zA-Z0-9_])"
        text = re.sub(pattern, raw_name, text)
    return text


# ============================================================================
# Pass 1a: Extract code lines from .text sections
# ============================================================================

def extract_all_code(lines):
    """
    Extract all code from the assembly file, spanning multiple functions.
    Returns a list of raw line strings inside .text sections.

    Strips: .globl, .type, .size, .p2align, .Lfunc_end markers.
    Keeps:  instructions, local/function labels, #APP/#NO_APP, comments.
    """
    code_lines = []
    in_code_section = False
    in_function = False

    skip_in_body = {".globl", ".type ", ".size ", ".p2align"}

    for line in lines:
        stripped = line.strip()

        if not stripped:
            continue
        if should_strip_directive(stripped):
            continue
        if is_strip_section(stripped):
            continue

        if stripped.startswith(".section"):
            in_code_section = ".text" in stripped
            continue

        if not in_code_section:
            continue

        skip = any(stripped.startswith(d) for d in skip_in_body)
        if skip:
            continue

        if stripped.startswith(".Lfunc_end"):
            in_function = False
            continue

        func_label_match = re.match(r"^([a-zA-Z_]\w*)\s*:", stripped)
        if func_label_match:
            in_function = True
            code_lines.append(stripped)
            continue

        if in_function:
            code_lines.append(stripped)

    return code_lines


# ============================================================================
# Pass 1b: Extract rodata objects from .rodata sections
#
# Returns a list of DataObject instances in source order.
# Each object represents one labeled symbol in a .rodata section.
# ============================================================================

class DataObject:
    def __init__(self, name, align, entries, section):
        self.name    = name
        self.align   = align      # alignment in bytes
        self.entries = entries    # list of (width_bytes, int_value)
        self.section = section    # original section string

    def __repr__(self):
        return f"DataObject({self.name!r}, align={self.align}, {len(self.entries)} entries)"


def _extract_raw_objects(lines):
    """
    Walk the file and collect all data objects from .rodata*, .bss*, and .data*
    sections.  Returns list of DataObject in source order.

    Handles:
      - .rodata*  sections: initialised read-only data (.byte/.half/.word etc.)
      - .bss*     sections: zero-initialised data (.zero N)
      - .data*    sections: initialised writable data (.byte/.half/.word etc.)
      - .set alias directives inside these sections are silently ignored
        (the aliased symbol shares storage with the primary label).
    """
    objects = []
    _set_aliases_out = {}     # filled by .set lines; read by caller via return value
    in_data_section = False   # True while inside a .rodata*, .bss*, or .data* section
    current_section = None
    current_obj = None
    pending_align = 1

    skip_obj_directives = {".globl", ".type ", ".size "}

    for line in lines:
        stripped = line.strip()

        if not stripped:
            continue
        if should_strip_directive(stripped):
            continue
        if is_strip_section(stripped):
            continue

        # .set alias, base[+offset] -- collect for later splitting.
        # These don't allocate storage; they name a sub-region of the parent.
        set_m = re.match(r'\.set\s+(\w+),\s*(\S+)', stripped)
        if set_m:
            alias  = set_m.group(1)
            target = set_m.group(2)
            off_m  = re.match(r'^([.\w]+)\+?(\d*)$', target)
            if off_m:
                base   = off_m.group(1)
                offset = int(off_m.group(2)) if off_m.group(2) else 0
                _set_aliases_out[alias] = (base, offset)
            continue

        # Section transitions
        if stripped.startswith(".section"):
            if current_obj is not None:
                objects.append(current_obj)
                current_obj = None

            m = re.match(r'\.section\s+([^\s,]+)', stripped)
            sec_name = m.group(1) if m else ""
            if ".rodata" in sec_name or ".bss" in sec_name or ".data" in sec_name:
                in_data_section = True
                current_section = sec_name
                pending_align = 1
            else:
                in_data_section = False
                current_section = None
            continue

        if not in_data_section:
            continue

        # Alignment hint before the label
        if stripped.startswith(".p2align"):
            m = re.match(r'\.p2align\s+(\d+)', stripped)
            if m:
                pending_align = 1 << int(m.group(1))
            continue

        # Skip structural directives
        if any(stripped.startswith(d) for d in skip_obj_directives):
            continue

        # Object label: starts a new DataObject.
        # Matches both plain names (foo:) and .L-prefixed locals (.L_MergedGlobals:).
        obj_label = re.match(r'^(\.L\w+|[a-zA-Z_]\w*)\s*:', stripped)
        if obj_label:
            if current_obj is not None:
                objects.append(current_obj)
            current_obj = DataObject(
                name    = obj_label.group(1),
                align   = pending_align,
                entries = [],
                section = current_section or ".rodata",
            )
            pending_align = 1
            continue

        # Data directives inside an object
        if current_obj is not None:
            # .zero N  -- emit N zero bytes (common in .bss sections)
            zero_m = re.match(r'\.zero\s+(\d+)', stripped)
            if zero_m:
                n_bytes = int(zero_m.group(1))
                for _ in range(n_bytes):
                    current_obj.entries.append((1, 0))
                continue

            for directive, width in DATA_DIRECTIVE_WIDTH.items():
                if stripped.startswith(directive):
                    rest = stripped[len(directive):].strip()
                    rest = re.sub(r'\s*#.*$', '', rest).strip()
                    for val_str in rest.split(','):
                        val_str = val_str.strip()
                        if val_str:
                            try:
                                val = int(val_str, 0)
                            except ValueError:
                                val = 0
                            current_obj.entries.append((width, val))
                    break

    if current_obj is not None:
        objects.append(current_obj)

    return objects, _set_aliases_out



def extract_rodata(lines):
    """
    Extract all data objects from .rodata*, .bss*, and .data* sections,
    then split each object at any .set alias offsets so that every aliased
    sub-region gets its own DataObject (and therefore its own numeric label).

    Returns (objects, alias_label_map) where:
      objects         -- list of DataObject in source order
      alias_label_map -- dict mapping (base_name, byte_offset) -> alias_name
                         for every .set directive found, used by replace_reloc
                         to resolve %hi(base+N) -> %hi(alias_label)
    """
    raw_objects, set_aliases = _extract_raw_objects(lines)

    # Build alias_label_map: (base_name, offset) -> alias_name
    # Also includes (base_name, 0) for the base label itself.
    alias_label_map = {}
    for alias, (base, offset) in set_aliases.items():
        alias_label_map[(base, offset)] = alias

    # For each DataObject that has aliases at non-zero offsets, split it.
    # We emit one DataObject per contiguous sub-region between split points.
    #
    # Example: .L_MergedGlobals (72 bytes), aliases at offset 0 (render_buf)
    # and offset 32 (led_buf) -> two DataObjects:
    #   DataObject(".L_MergedGlobals", entries[0:32])   <- 32 zero bytes
    #   DataObject("led_buf",          entries[32:72])  <- 40 zero bytes
    #
    # The base object keeps its original name (first split point = offset 0),
    # subsequent splits use the alias name.
    result_objects = []

    for obj in raw_objects:
        # Find all aliases that refer to this object, sorted by offset
        splits = sorted(
            (offset, alias)
            for (base, offset), alias in alias_label_map.items()
            if base == obj.name
        )

        # Always include offset 0 under the object's own name if not already there
        split_offsets = {off for off, _ in splits}
        if 0 not in split_offsets:
            splits = [(0, obj.name)] + splits
        else:
            # Replace the offset-0 alias with the original object name
            splits = [(off, (obj.name if off == 0 else alias)) for off, alias in splits]

        splits.sort()

        if len(splits) <= 1:
            # No meaningful splits -- emit as-is under original name
            result_objects.append(obj)
            continue

        # Convert entries to a flat byte list for slicing
        byte_list = []
        for width, val in obj.entries:
            mask = (1 << (width * 8)) - 1
            v = val & mask
            for _ in range(width):
                byte_list.append(v & 0xFF)
                v >>= 8

        total_bytes = len(byte_list)

        for i, (start_off, name) in enumerate(splits):
            end_off = splits[i + 1][0] if i + 1 < len(splits) else total_bytes
            chunk = byte_list[start_off:end_off]
            # Re-encode as (1, byte) entries
            entries = [(1, b) for b in chunk]
            result_objects.append(DataObject(
                name    = name,
                align   = obj.align if start_off == 0 else 1,
                entries = entries,
                section = obj.section,
            ))

    return result_objects, alias_label_map

# ============================================================================
# Pack data entries into .word (32-bit) lines
#
# All entries are packed little-endian byte by byte, then grouped into
# 32-bit words.  Padding zeros are added if the byte total is not
# a multiple of 4.
# ============================================================================

def pack_to_words(entries):
    """
    Convert a list of (width_bytes, int_value) entries into a list of
    32-bit unsigned integers, little-endian packed.
    Entries are packed little-endian byte by byte, then grouped into 32-bit words.
    """
    byte_buf = []
    for width, val in entries:
        mask = (1 << (width * 8)) - 1
        v = val & mask
        for _ in range(width):
            byte_buf.append(v & 0xFF)
            v >>= 8

    # Pad to multiple of 4
    while len(byte_buf) % 4 != 0:
        byte_buf.append(0)

    words = []
    for i in range(0, len(byte_buf), 4):
        w = (byte_buf[i]
             | (byte_buf[i+1] << 8)
             | (byte_buf[i+2] << 16)
             | (byte_buf[i+3] << 24))
        words.append(w)
    return words



# ============================================================================
# Label collection and unified mapping
# ============================================================================

def collect_all_labels(code_lines):
    """
    Collect all labels defined in the code lines.
    Returns dict: label_name -> line_index
    """
    labels = {}
    for i, line in enumerate(code_lines):
        m = re.match(r"^(\.L\w+)\s*:", line)
        if m:
            labels[m.group(1)] = i
            continue
        m = re.match(r"^([a-zA-Z_]\w*)\s*:", line)
        if m:
            labels[m.group(1)] = i
            continue
    return labels


def build_label_map(code_labels, data_objects, label_base):
    """
    Assign a unique numeric label to every code label and every data object
    name.  Code labels are ordered by source position; data labels follow
    in source order.  Data labels always get a higher number than code labels
    because they appear after the code block in the macro output.

    Returns label_map: name -> int
    """
    label_map = {}
    n = label_base
    for name in sorted(code_labels.keys(), key=lambda k: code_labels[k]):
        label_map[name] = n
        n += 1
    for obj in data_objects:
        label_map[obj.name] = n
        n += 1
    return label_map


# ============================================================================
# Pass 2: Convert individual code lines
# ============================================================================

def convert_line(line, label_map, label_positions, current_line_idx, alias_label_map=None):
    """
    Convert a single code line to its Rust inline-asm string equivalent.
    Returns (instruction_string_or_None, comment_or_None).

    label_positions must contain an entry for every name in label_map.
    Data labels should be mapped to len(code_lines) so they always appear
    as forward references from any code position.
    """
    stripped = line.strip()

    if stripped == "#APP":
        return None, "// --- inline asm begin ---"
    if stripped == "#NO_APP":
        return None, "// --- inline asm end ---"

    # Label definition
    label_def_match = re.match(r"^(\.L\w+|[a-zA-Z_]\w*)\s*:\s*(.*)", stripped)
    if label_def_match:
        label_name = label_def_match.group(1)
        rest = label_def_match.group(2).strip()
        if label_name in label_map:
            num = label_map[label_name]
            result = f"{num}:"
            trailing_comment = None
            if rest:
                comment_m = re.match(r"^#\s*(.*)", rest)
                if comment_m:
                    trailing_comment = f"// {label_name}: {comment_m.group(1).strip()}"
                else:
                    result += f" {rest}"
            if not trailing_comment:
                trailing_comment = f"// {label_name}"
            return result, trailing_comment
        return stripped, None

    # Pure comment / block marker lines
    if stripped.startswith("#") or stripped.startswith("//"):
        bb_match = re.match(r"^#\s*%bb\.\d+", stripped)
        if bb_match:
            rest = re.sub(r"^#\s*%bb\.\d+:?\s*", "", stripped).strip()
            rest = re.sub(r"^#\s*", "", rest).strip()
            if rest:
                return None, f"// {rest}"
            return None, None
        comment_text = re.sub(r"^[#/]+\s*", "", stripped).strip()
        if comment_text:
            return None, f"// {comment_text}"
        return None, None

    # Instruction line: split off trailing comment
    comment = None
    instr = stripped
    comment_match = re.search(r"\s+#\s*(.*)", instr)
    if comment_match:
        comment = f"// {comment_match.group(1).strip()}"
        instr = instr[: comment_match.start()].strip()

    # Replace %hi(label) and %lo(label) with numeric forward-ref equivalents.
    # Also handles %lo(label+offset) and %hi(label+offset) forms produced by
    # clang for symbols like ".L_MergedGlobals+4".
    # Data labels are always forward refs; code labels use position comparison.
    def replace_reloc(m):
        kind   = m.group(1)
        lname  = m.group(2)
        offset_str = m.group(3) or ""  # e.g. "+4", "-8", or ""
        offset_val = int(offset_str) if offset_str else 0

        # If this is a base+offset reference and we have a .set alias that
        # lands exactly at that offset, resolve directly to the alias label.
        # This avoids emitting %hi(Nf+32) which is invalid in Rust inline asm.
        if alias_label_map and offset_val != 0:
            alias = alias_label_map.get((lname, offset_val))
            if alias and alias in label_map:
                num  = label_map[alias]
                lpos = label_positions.get(alias, current_line_idx + 1)
                ref  = f"{num}f" if lpos > current_line_idx else f"{num}b"
                return f"%{kind}({ref})"   # no offset -- alias IS the label

        if lname in label_map:
            num  = label_map[lname]
            lpos = label_positions.get(lname, current_line_idx + 1)
            ref  = f"{num}f" if lpos > current_line_idx else f"{num}b"
            if offset_str:
                # No alias covers this offset -- warn and emit bare ref
                # (the +N form is invalid in inline asm; caller should add
                # -fno-merge-globals or restructure to avoid this)
                WARNINGS.append(
                    f"WARNING: %{kind}({lname}{offset_str}) has no .set alias -- "
                    f"emitting bare %{kind}({ref}), offset {offset_str} dropped"
                )
            return f"%{kind}({ref})"
        return m.group(0)

    # Pattern: %reloc(label_name[+/-offset]?)
    #   group 1: reloc kind  (hi, lo, ...)
    #   group 2: label name  (may start with .L)
    #   group 3: optional offset suffix (+4, -8, ...)
    instr = re.sub(
        r'%(\w+)\((\.L\w+|[a-zA-Z_]\w*)([+\-]\d+)?\)',
        replace_reloc,
        instr,
    )

    # Special-case `call` and `tail` pseudo-instructions.
    # These require a *bare symbol name*, so numeric local-label references
    # like "27b" are rejected by the assembler.  When the target is a known
    # label we expand:
    #   call  foo  ->  jal  ra, NNf/b
    #   tail  foo  ->  jal  zero, NNf/b   (tail-call: no return-address save)
    # Both pseudo-instructions assemble to two real instructions in the
    # general case, but within the tight range of a single bio_code block
    # the single-instruction jal is always sufficient and correct.
    call_tail_match = re.match(
        r'^(call|tail)\s+([a-zA-Z_]\w*)\s*$', instr, re.IGNORECASE
    )
    if call_tail_match:
        pseudo    = call_tail_match.group(1).lower()
        lname     = call_tail_match.group(2)
        if lname in label_map:
            num  = label_map[lname]
            lpos = label_positions.get(lname, current_line_idx + 1)
            ref  = f"{num}f" if lpos > current_line_idx else f"{num}b"
            rd   = "ra" if pseudo == "call" else "zero"
            instr = f"jal\t{rd}, {ref}"
            # comment already captured; skip generic label replacement
            instr = replace_registers(instr)
            return instr, comment

    # Replace plain label references (branches, jumps, address loads).
    # Longest names first to avoid partial matches.
    for label_name in sorted(label_map.keys(), key=len, reverse=True):
        num = label_map[label_name]
        if label_name not in instr:
            continue
        lpos = label_positions.get(label_name, current_line_idx + 1)
        ref  = f"{num}f" if lpos > current_line_idx else f"{num}b"
        pattern = r"(?<![a-zA-Z0-9_.])" + re.escape(label_name) + r"(?![a-zA-Z0-9_])"
        instr = re.sub(pattern, ref, instr)

    instr = replace_registers(instr)
    return instr, comment


# ============================================================================
# Pass 3: BIO errata patching
#
# Two classes of hardware bugs in the BIO decoder:
#
# BUG 1 ("phantom rs1"): For lui/auipc/jal, the 5-bit field at
#   instruction bits [19:15] is not gated off. If that field decodes
#   to register 16-19 (x16-x19) for just these instruction types,
#   a spurious pending read is made from the corresponding FIFO.
#   For U-type (lui/auipc), bits [19:15] = imm[7:3].
#   For J-type (jal), bits [19:15] = imm[10:6] (after J-encoding shuffle).
#   Fix: decompose the immediate so the problematic bits are avoided,
#   using a temp register saved/restored via stack.
#
# BUG 2 ("phantom rs2"): For non-R/S/B-type instructions, the 5-bit
#   field at instruction bits [24:20] should be gated off but isn't.
#   When that field equals 20 (0b10100), a spurious `quantum` signal
#   triggers, which can affect the instruction fetch pipeline.
#   Affected formats:
#     - I-type (addi, andi, ori, etc.): bits [24:20] = imm[4:0]
#     - I-type loads (lw, lh, lb, etc.): bits [24:20] = offset[4:0]
#     - I-type shifts (slli, srli, srai): bits [24:20] = shamt[4:0]
#     - U-type (lui, auipc): bits [24:20] = imm[8:4]
#     - J-type (jal): bits [24:20] = imm[4:0] (after J-encoding shuffle)
#   Fix: varies by instruction type - decompose shifts, adjust offsets
#   for loads/stores, split immediates for ALU ops, etc.
#
# The patching runs after convert_line (Pass 2) and before format_output.
# Each instruction is parsed, checked against the dispatch table, and
# potentially expanded into a multi-instruction sequence.
# ============================================================================

# Temp register used for errata workarounds.  We save/restore via stack.
# Stack save/restore is safe so long as:
#   - The stack is set up (which it is by the C entry point)
#   - There are no interrupts or concurrency (and no interrupts are possible
#     in this implementation as they are not implemented)
ERRATA_TEMP = "t0"
ERRATA_STACK_OFFSET = -4  # known safe: lower 5 bits = 0b11100, not 20; will not recursively trigger Bug 2

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
    if re.match(r'^\d+:', s):
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
    Also handles "%lo(32f)(a5)" style - returns None for those (relocation).
    """
    m = re.match(r'^(-?\d+)\((\w+)\)$', operand)
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


# --------------------------------------------------------------------------
# Bug 1: U/J-type "phantom rs1" in bits [19:15]
# --------------------------------------------------------------------------

# The problematic register numbers for Bug 1
BUG1_BAD_REGS = {16, 17, 18, 19}

def _u_type_phantom_rs1(imm20):
    """
    For U-type (lui/auipc), check if bits [19:15] of the encoded instruction
    would decode to a problematic register.
    U-type encoding: imm[19:0] is placed in instruction bits [31:12].
    So instruction bits [19:15] = imm[7:3].
    """
    field = (imm20 >> 3) & 0x1F
    return field in BUG1_BAD_REGS


def _j_type_phantom_rs1(imm21):
    """
    For J-type (jal), check bits [19:15] of the encoded instruction.
    J-type encoding shuffles the immediate:
      inst[31]    = imm[20]
      inst[30:21] = imm[10:1]
      inst[20]    = imm[11]
      inst[19:12] = imm[19:12]
    So instruction bits [19:15] = imm[19:15].
    """
    field = (imm21 >> 15) & 0x1F
    return field in BUG1_BAD_REGS


def _u_type_phantom_rs2(imm20):
    """
    Bug 2 for U-type: check bits [24:20] of the encoded instruction.
    inst bits [24:20] = imm[12:8].
    """
    field = (imm20 >> 8) & 0x1F
    return field == 20


def _j_type_phantom_rs2(imm21):
    """
    Bug 2 for J-type (jal): check bits [24:20] of the encoded instruction.
    J-type: inst[30:21] = imm[10:1], inst[20] = imm[11]
    So inst bits [24:20] = {imm[3:1], imm[11], imm[20]}... actually let me
    work this out properly.
    inst[24] = imm[4], inst[23] = imm[3], inst[22] = imm[2], inst[21] = imm[1],
    inst[20] = imm[11].
    So the 5-bit field = (imm[4:1] << 1) | imm[11].
    Wait, no. inst[30:21] = imm[10:1]. So:
      inst[24] = imm[4]
      inst[23] = imm[3]
      inst[22] = imm[2]
      inst[21] = imm[1]
      inst[20] = imm[11]
    field = (imm >> 1) & 0xF  -> gives imm[4:1], shift left 1
    field = ((imm >> 1) & 0xF) << 1 | ((imm >> 11) & 1)
    """
    field = (((imm21 >> 1) & 0xF) << 1) | ((imm21 >> 11) & 1)
    return field == 20


# --------------------------------------------------------------------------
# Bug 2: bits [24:20] = 20 for I-type instructions
# --------------------------------------------------------------------------

def _i_type_imm_has_bug2(imm12):
    """
    For I-type, bits [24:20] = imm[4:0].
    Check if lower 5 bits of the sign-extended 12-bit immediate = 20.
    """
    # Sign extend to get the actual encoding bits
    if imm12 < 0:
        encoded = imm12 & 0xFFF
    else:
        encoded = imm12 & 0xFFF
    return (encoded & 0x1F) == 20


# --------------------------------------------------------------------------
# Patch generation helpers
# --------------------------------------------------------------------------

def _save_temp():
    """Emit instruction to save temp register to stack."""
    return f"sw {ERRATA_TEMP}, {ERRATA_STACK_OFFSET}(sp)"

def _restore_temp():
    """Emit instruction to restore temp register from stack."""
    return f"lw {ERRATA_TEMP}, {ERRATA_STACK_OFFSET}(sp)"


def _make_errata_comment(original, bug_id, detail=""):
    """Create a comment describing the errata patch."""
    extra = f" ({detail})" if detail else ""
    return f"// ERRATA BUG{bug_id} patch: {original}{extra}"

def _build_constant_in_reg(temp_reg, diff):
    """
    Build instructions to load `diff` into temp_reg, where diff is the
    bits we cleared from a U-type immediate.

    Returns (safe_lui_imm, [instructions to build diff in temp_reg]).
    The caller does: lui rd, safe_lui_imm; then these instructions; then add rd, rd, temp_reg.

    We need to be careful that our generated instructions don't themselves
    trigger the bugs.
    """
    instrs = []

    if diff == 0:
        # Nothing to fix up - the safe_base is already correct
        return []

    # Find the lowest set bit and highest set bit of diff
    # to determine the best shift strategy
    lowest_bit = (diff & -diff).bit_length() - 1  # position of lowest set bit
    shifted_val = diff >> lowest_bit  # the core value we need to create

    # Strategy: load shifted_val with lui/addi if small enough, then shift left.
    #   NB: shifted_val itself must not trigger bugs when loaded.
    # Since shifted_val comes from cleared fields of a 20-bit value, it should
    # be small enough for addi (12-bit signed: -2048..2047).
    if shifted_val < 2048:
        # addi temp, zero, shifted_val -- check this doesn't trigger bug 2
        if _i_type_imm_has_bug2(shifted_val):
            # Split: load (shifted_val - 1), then addi 1
            instrs.append(f"addi {temp_reg}, zero, {shifted_val - 1}")
            instrs.append(f"addi {temp_reg}, {temp_reg}, 1")
        else:
            instrs.append(f"addi {temp_reg}, zero, {shifted_val}")

        # Now shift left to get the full diff
        # Be careful: slli with shamt=20 triggers bug 2!
        remaining_shift = lowest_bit
        while remaining_shift > 0:
            if remaining_shift >= 20:
                # Can't do slli by 20, split into 19 + remainder
                instrs.append(f"slli {temp_reg}, {temp_reg}, 19")
                remaining_shift -= 19
            elif _i_type_imm_has_bug2(remaining_shift):
                # remaining_shift has lower 5 bits = 20, split
                instrs.append(f"slli {temp_reg}, {temp_reg}, {remaining_shift - 1}")
                instrs.append(f"slli {temp_reg}, {temp_reg}, 1")
                remaining_shift = 0
            else:
                instrs.append(f"slli {temp_reg}, {temp_reg}, {remaining_shift}")
                remaining_shift = 0
    else:
        # Larger value - use lui for the shifted_val itself
        # This is a recursive-ish problem, but in practice diff should be small
        # since we only cleared 5 or 10 bits from a 20-bit value.
        # Fallback: build bit by bit with shifts and ORs.
        # For now, use addi with sign extension tricks.
        # shifted_val fits in 20 bits since diff is 20 bits.
        if shifted_val < (1 << 20):
            # Use lui + addi
            upper = shifted_val >> 12
            lower = shifted_val & 0xFFF
            if lower >= 0x800:
                upper += 1
                lower = lower - 0x1000  # sign-extend correction
            # Check upper for bug 1/2
            if not _u_type_phantom_rs1(upper) and not _u_type_phantom_rs2(upper):
                instrs.append(f"lui {temp_reg}, {upper}")
                if lower != 0:
                    if _i_type_imm_has_bug2(lower):
                        instrs.append(f"addi {temp_reg}, {temp_reg}, {lower - 1}")
                        instrs.append(f"addi {temp_reg}, {temp_reg}, 1")
                    else:
                        instrs.append(f"addi {temp_reg}, {temp_reg}, {lower}")
            else:
                # Extremely unlikely edge case - just use small steps
                instrs.append(f"addi {temp_reg}, zero, {(shifted_val >> 10) & 0x7FF}")
                instrs.append(f"slli {temp_reg}, {temp_reg}, 10")
                rest = shifted_val & 0x3FF
                if rest:
                    instrs.append(f"addi {temp_reg}, {temp_reg}, {rest}")

            # Shift left
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


# --------------------------------------------------------------------------
# Instruction-specific patch functions
#
# Each returns either None (no patch needed) or a list of
# (instruction_str, comment_str_or_None) tuples to replace the original.
# --------------------------------------------------------------------------

def _patch_lui(mnemonic, operands, original_instr):
    """Patch lui rd, imm20 for Bug 1 and Bug 2."""
    if len(operands) != 2:
        return None
    rd = operands[0]
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None  # relocation or label ref, can't analyze statically

    imm20 = imm & 0xFFFFF

    has_bug1 = _u_type_phantom_rs1(imm20)
    has_bug2 = _u_type_phantom_rs2(imm20)

    if not has_bug1 and not has_bug2:
        return None  # safe

    bugs = []
    if has_bug1:
        bugs.append("B1:phantom-rs1")
    if has_bug2:
        bugs.append("B2:bits24:20=20")
    bug_desc = "+".join(bugs)

    # Build fixup instructions
    safe_imm20 = imm20
    if has_bug1:
        safe_imm20 = safe_imm20 & ~(0x1F << 3)
    if has_bug2:
        safe_imm20 = safe_imm20 & ~(0x1F << 8)

    diff = (imm20 - safe_imm20) & 0xFFFFF
    fixup_instrs = _build_constant_in_reg(ERRATA_TEMP, diff)

    result = []
    comment = _make_errata_comment(original_instr, "1+2" if (has_bug1 and has_bug2) else ("1" if has_bug1 else "2"), bug_desc)
    result.append((None, comment))

    # Step 1: lui with safe immediate (may be 0, that's fine)
    if safe_imm20 != 0:
        result.append((f"lui {rd}, 0x{safe_imm20:x}", None))
    else:
        # If safe immediate is 0, just start with zero in rd
        result.append((f"addi {rd}, zero, 0", None))

    # Step 2: save temp, build fixup, add, restore
    if diff != 0:
        result.append((_save_temp(), None))
        for fi in fixup_instrs:
            result.append((fi, None))
        # The fixup value in ERRATA_TEMP is the raw diff; since lui shifts
        # by 12, and our diff is in the "lui immediate" domain, we need to
        # shift it left by 12 to match what lui would have placed.
        result.append((f"slli {ERRATA_TEMP}, {ERRATA_TEMP}, 12", None))
        result.append((f"add {rd}, {rd}, {ERRATA_TEMP}", None))
        result.append((_restore_temp(), None))

    return result


def _patch_auipc(mnemonic, operands, original_instr):
    """
    Patch auipc rd, imm20 for Bug 1 and Bug 2.
    auipc is trickier because it adds to PC. We can't simply decompose
    the immediate because the PC-relative semantics change.

    Strategy: Use a safe auipc with zeroed problematic bits, then fix up
    the difference using temp register arithmetic.
    The fixup is a constant (diff << 12) added to rd.
    """
    if len(operands) != 2:
        return None
    rd = operands[0]
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None  # relocation, can't patch statically

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

    result = []
    comment = _make_errata_comment(original_instr, "1+2" if (has_bug1 and has_bug2) else ("1" if has_bug1 else "2"), bug_desc)
    result.append((None, comment))

    # auipc with safe immediate - this captures PC correctly
    result.append((f"auipc {rd}, 0x{safe_imm20:x}", None))

    if diff != 0:
        # Add the missing (diff << 12) using temp
        result.append((_save_temp(), None))
        fixup_instrs = _build_constant_in_reg(ERRATA_TEMP, diff)
        for fi in fixup_instrs:
            result.append((fi, None))

        result.append((f"slli {ERRATA_TEMP}, {ERRATA_TEMP}, 12", None))
        result.append((f"add {rd}, {rd}, {ERRATA_TEMP}", None))
        result.append((_restore_temp(), None))

    return result


def _patch_jal(mnemonic, operands, original_instr):
    """
    Patch jal rd, offset for Bug 1 and Bug 2.
    jal is PC-relative so we can't trivially decompose.

    Simply emit a warning when this case is encountered. It is only triggered when
    when the final binary output - after the labels are resolved - has a value
    of 20 in the immediate field, e.g. "jal x1, 20" (*not* '20f', literally the value
    20).

    This is a rare edge case - emit a warning to review - if it triggers,
    the fix is to just insert a NOP after the JAL instruction, and
    the label will have shifted to avoid triggering the bug.
    """
    # jal can be "jal rd, offset" or pseudo "jal offset" (rd=ra)
    if len(operands) == 1:
        # Pseudo form: jal target
        return None  # likely a label, can't analyze
    if len(operands) != 2:
        return None
    rd = operands[0]
    imm = _imm_to_int(operands[1])
    if imm is None:
        return None  # label reference, can't analyze statically

    imm21 = imm & 0x1FFFFF

    has_bug1 = _j_type_phantom_rs1(imm21)
    has_bug2 = _j_type_phantom_rs2(imm21)

    if not has_bug1 and not has_bug2:
        return None

    # For jal with a numeric offset that hits the bug, replace with
    # auipc + jalr (which are I-type and won't trigger bug 1).
    # jalr rd, offset(temp) -- but we need to compute the right split.
    # Actually since jal is rare with numeric immediates in compiler output,
    # just emit a warning comment for now.
    bugs = []
    if has_bug1:
        bugs.append("B1:phantom-rs1")
    if has_bug2:
        bugs.append("B2:bits24:20=20")
    bug_desc = "+".join(bugs)

    result = []
    result.append((None, _make_errata_comment(original_instr, "1/2", bug_desc)))
    result.append((None, "// WARNING: jal with numeric immediate - manual review needed"))
    result.append((original_instr, None))
    WARNINGS.append("JAL with numeric intermediate detected - manual review for type 2 bug needed")
    return result


def _patch_shift_imm(mnemonic, operands, original_instr):
    """
    Patch slli/srli/srai rd, rs1, shamt where shamt's encoding triggers Bug 2.
    For shifts, bits [24:20] = shamt. Bug triggers when shamt & 0x1F == 20.
    For RV32, shamt is 0-31, so this means shamt == 20.

    Fix: split into two shifts that sum to the original shamt.
    e.g. slli a0, a0, 20 -> slli a0, a0, 19; slli a0, a0, 1
    """
    if len(operands) != 3:
        return None
    rd, rs1, shamt_str = operands
    shamt = _imm_to_int(shamt_str)
    if shamt is None:
        return None
    if (shamt & 0x1F) != 20:
        return None

    result = []
    result.append((None, _make_errata_comment(original_instr, "2", f"shamt={shamt}")))
    # Split: (shamt - 1) + 1.  shamt-1 = 19, which is safe (19 & 0x1F = 19 != 20)
    result.append((f"{mnemonic} {rd}, {rs1}, {shamt - 1}", None))
    result.append((f"{mnemonic} {rd}, {rd}, 1", None))
    return result


def _patch_load(mnemonic, operands, original_instr):
    """
    Patch load instructions (lw/lh/lb/lhu/lbu) where the offset triggers Bug 2.
    Load is I-type: bits [24:20] = offset[4:0].
    Bug triggers when (offset_encoding & 0x1F) == 20.

    Fix: adjust the base register by -4, use offset+4 (which changes the
    lower 5 bits), then restore the base register.
    We use addi on the base register directly (no temp needed for this one).

    If base == rd, we don't restore the base register.
    """
    if len(operands) != 2:
        return None
    rd = operands[0]
    offset, base = _parse_mem_operand(operands[1])
    if offset is None:
        return None  # relocation or unparseable

    offset_enc = offset & 0xFFF
    if (offset_enc & 0x1F) != 20:
        return None

    # Find an adjustment that makes the new offset safe.
    # Try small adjustments: -4, +4, -8, +8, etc.
    # Pretty sure that -4 covers all the cases, but might as well check just in case.
    for adj in [-4, 4, -8, 8, -12, 12, -16, 16]:
        new_offset = offset - adj  # because we addi base by +adj, effective offset decreases
        # Original: lw rd, offset(base)  -> rd = mem[base + offset]
        # Patched:  addi base, base, delta
        #           lw rd, (offset - delta)(base)  -> rd = mem[base + delta + offset - delta] = mem[base + offset]
        #           addi base, base, -delta
        new_offset = offset - adj
        new_offset_enc = new_offset & 0xFFF
        if (new_offset_enc & 0x1F) != 20:
            # Also check that adj and -adj are safe for addi
            adj_enc = adj & 0xFFF
            neg_adj_enc = (-adj) & 0xFFF
            if (adj_enc & 0x1F) != 20 and (neg_adj_enc & 0x1F) != 20:
                # Also check the new offset is within I-type range (-2048..2047)
                if -2048 <= new_offset <= 2047:
                    delta = adj
                    break
    else:
        # Shouldn't happen with the range of adjustments we try, but fallback
        delta = -4
        new_offset = offset + 4

    result = []
    result.append((None, _make_errata_comment(original_instr, "2", f"offset={offset},bits[4:0]={offset_enc & 0x1F}")))

    if rd == base:
        # rd == base: the value is clobbered, so we don't need to restore the delta
        result.append((f"addi {ERRATA_TEMP}, {base}, {delta}", None))
        result.append((f"{mnemonic} {rd}, {new_offset}({ERRATA_TEMP})", None))
    else:
        result.append((f"addi {base}, {base}, {delta}", None))
        result.append((f"{mnemonic} {rd}, {new_offset}({base})", None))
        result.append((f"addi {base}, {base}, {-delta}", None))

    return result


def _patch_store(mnemonic, operands, original_instr):
    """
    Patch store instructions (sw/sh/sb) where the offset triggers Bug 2.
    Store is S-type: bits [24:20] = rs2 (the source register).
    Since bits [24:20] in S-type is a register field, and the hardware
    correctly handles register encodings in R/S/B-type, stores do NOT
    trigger Bug 2 via the rs2 field.

    However, we should still check: the S-type immediate is split across
    bits [31:25] (imm[11:5]) and bits [11:7] (imm[4:0]). Neither of these
    overlaps with bits [24:20], so the offset itself doesn't cause Bug 2.

    Therefore: stores don't need Bug 2 patching. This function exists
    as a dispatch entry that returns None (no patch) for documentation.
    """
    return None


def _patch_i_type_alu(mnemonic, operands, original_instr):
    """
    Patch I-type ALU instructions (addi, andi, ori, xori, slti, sltiu)
    where the immediate triggers Bug 2.
    I-type: bits [24:20] = imm[4:0]. Bug when (imm_encoding & 0x1F) == 20.

    Fix depends on the operation:
    - addi: split into two adds, e.g. addi rd, rs1, 20 -> addi rd, rs1, 19; addi rd, rd, 1
    - andi: need temp. andi rd, rs1, imm -> save temp; addi temp, zero, imm; and rd, rs1, temp; restore
    - ori:  same as andi but with or
    - xori: same with xor
    - slti/sltiu: need temp. use addi to load imm into temp, then slt/sltu.
    """
    if len(operands) != 3:
        return None
    rd, rs1, imm_str = operands
    imm = _imm_to_int(imm_str)
    if imm is None:
        return None

    imm_enc = imm & 0xFFF
    if (imm_enc & 0x1F) != 20:
        return None

    result = []
    result.append((None, _make_errata_comment(original_instr, "2", f"imm={imm},bits[4:0]={imm_enc & 0x1F}")))

    if mnemonic == 'addi':
        # Split: addi rd, rs1, (imm-1); addi rd, rd, 1
        new_imm = imm - 1
        new_enc = new_imm & 0xFFF
        if (new_enc & 0x1F) == 20:
            # Very unlikely but handle: use +1 then -2 split instead
            new_imm = imm + 1
            result.append((f"addi {rd}, {rs1}, {new_imm}", None))
            result.append((f"addi {rd}, {rd}, -1", None))
        else:
            result.append((f"addi {rd}, {rs1}, {new_imm}", None))
            result.append((f"addi {rd}, {rd}, 1", None))
    elif mnemonic in ('andi', 'ori', 'xori'):
        # Load immediate into temp, use R-type operation
        r_type_op = {'andi': 'and', 'ori': 'or', 'xori': 'xor'}[mnemonic]
        result.append((_save_temp(), None))
        # Build the immediate in temp - addi from zero
        if (imm_enc & 0x1F) == 20:
            # imm itself triggers bug 2 in addi, so split the load
            result.append((f"addi {ERRATA_TEMP}, zero, {imm - 1}", None))
            result.append((f"addi {ERRATA_TEMP}, {ERRATA_TEMP}, 1", None))
        else:
            result.append((f"addi {ERRATA_TEMP}, zero, {imm}", None))
        result.append((f"{r_type_op} {rd}, {rs1}, {ERRATA_TEMP}", None))
        result.append((_restore_temp(), None))
    elif mnemonic in ('slti', 'sltiu'):
        r_type_op = {'slti': 'slt', 'sltiu': 'sltu'}[mnemonic]
        result.append((_save_temp(), None))
        result.append((f"addi {ERRATA_TEMP}, zero, {imm - 1}", None))
        result.append((f"addi {ERRATA_TEMP}, {ERRATA_TEMP}, 1", None))
        result.append((f"{r_type_op} {rd}, {rs1}, {ERRATA_TEMP}", None))
        result.append((_restore_temp(), None))
    else:
        WARNINGS.append(f"Unhandled I-type opcode: {mnemonic}; manual review needed!")
        return None  # unknown I-type ALU, skip

    return result


def _patch_jalr(mnemonic, operands, original_instr):
    """
    Patch jalr rd, offset(rs1) where offset triggers Bug 2.
    jalr is I-type: bits [24:20] = offset[4:0].
    Bug when (offset_encoding & 0x1F) == 20.

    Fix: adjust rs1, use modified offset, restore rs1.
    Same approach as load patching.
    Edge case: rd == rs1 means we need temp register approach.
    """
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

    result = []
    result.append((None, _make_errata_comment(original_instr, "2", f"offset={offset}")))

    if rd == rs1:
        # Can't modify rs1 and restore it (jalr changes rd which is also rs1)
        result.append((_save_temp(), None))
        result.append((f"addi {ERRATA_TEMP}, {rs1}, {delta}", None))
        result.append((f"jalr {rd}, {new_offset}({ERRATA_TEMP})", None))
        result.append((_restore_temp(), None))
    else:
        result.append((f"addi {rs1}, {rs1}, {delta}", None))
        result.append((f"jalr {rd}, {new_offset}({rs1})", None))
        result.append((f"addi {rs1}, {rs1}, {-delta}", None))

    return result


# --------------------------------------------------------------------------
# Dispatch table
# --------------------------------------------------------------------------

ERRATA_DISPATCH = {
    # U-type: Bug 1 (phantom rs1) + Bug 2 (phantom rs2)
    'lui':    _patch_lui,
    'auipc':  _patch_auipc,

    # J-type: Bug 1 + Bug 2 - actually just emits warnings
    'jal':    _patch_jal,

    # I-type shifts: Bug 2 (shamt in bits [24:20])
    'slli':   _patch_shift_imm,
    'srli':   _patch_shift_imm,
    'srai':   _patch_shift_imm,

    # I-type loads: Bug 2 (offset[4:0] in bits [24:20])
    'lw':     _patch_load,
    'lh':     _patch_load,
    'lb':     _patch_load,
    'lhu':    _patch_load,
    'lbu':    _patch_load,

    # I-type ALU: Bug 2 (imm[4:0] in bits [24:20])
    'addi':   _patch_i_type_alu,
    'andi':   _patch_i_type_alu,
    'ori':    _patch_i_type_alu,
    'xori':   _patch_i_type_alu,
    'slti':   _patch_i_type_alu,
    'sltiu':  _patch_i_type_alu,

    # I-type jump: Bug 2 (offset[4:0] in bits [24:20])
    'jalr':   _patch_jalr,

    # S-type stores: bits [24:20] = rs2 (register), hardware handles correctly
    'sw':     _patch_store,
    'sh':     _patch_store,
    'sb':     _patch_store,
}


def apply_errata_patches(converted_lines):
    """
    Pass 3: Walk the converted instruction list and apply errata patches.

    Input:  list of (instruction_str_or_None, comment_or_None)
    Output: new list with problematic instructions expanded into safe sequences.

    Also returns a count of patches applied for diagnostics.
    """
    patched = []
    patch_count = 0

    for instr, comment in converted_lines:
        if instr is None:
            patched.append((instr, comment))
            continue

        # Skip instructions with relocations - can't analyze statically
        if _has_relocation(instr):
            patched.append((instr, comment))
            continue

        mnemonic, operands = _parse_instruction(instr)
        if mnemonic is None:
            patched.append((instr, comment))
            continue

        patch_fn = ERRATA_DISPATCH.get(mnemonic)
        if patch_fn is None:
            # Not in dispatch table - pass through
            patched.append((instr, comment))
            continue

        patch_result = patch_fn(mnemonic, operands, instr)
        if patch_result is None:
            # Checked, no bug triggered
            patched.append((instr, comment))
        else:
            # Replace with patched sequence
            # Preserve the original comment on the first real instruction
            for pi, (p_instr, p_comment) in enumerate(patch_result):
                if pi == 0 and p_instr is None and comment:
                    # Merge original comment with errata comment
                    merged = f"{p_comment}  {comment}" if p_comment else comment
                    patched.append((None, merged))
                else:
                    patched.append((p_instr, p_comment))
            patch_count += 1

    return patched, patch_count


# ============================================================================
# Format the final macro output
# ============================================================================

def p2align_power(align_bytes):
    """Convert an alignment in bytes to a .p2align exponent."""
    if align_bytes <= 1:
        return 0
    return (align_bytes).bit_length() - 1


def format_output(fn_name, start_label, end_label, converted_lines, data_objects, label_map):
    """
    Emit the complete bio_code! macro invocation.

    Layout inside the macro:
      1. Code instructions (with numeric labels inline)
      2. For each rodata object:
           .section <name>
           .p2align <N>
           <num>:          <- numeric label for the object
           .word 0x...     <- packed 32-bit words
    """
    parts = []
    # C-sourced routines use the "aligned" format for BIO libraries - just in case
    # absolute references are made to code, as would be the case in a `static` variable.
    # C-routines that are `static`-free could use `bio_code` and save the alignment
    # overhead, but the cost of confusing new programmers is not worth the savings
    # of a few kiB of code space.
    parts.append(f"// AUTOGENERATED CODE on {datetime.now().isoformat()} - do not edit directly!\n")
    parts.append("use bao1x_api::bio_code_aligned;\n")
    parts.append("#[rustfmt::skip]")
    parts.append(f"bio_code_aligned!({fn_name}, {start_label}, {end_label},")

    # ---- Code ----
    filtered = [(instr, comment) for instr, comment in converted_lines
                if instr is not None or comment is not None]

    for i, (instr, comment) in enumerate(filtered):
        is_last_instr = (instr is not None) and all(
            fi is None for fi, _ in filtered[i + 1:]
        )

        if instr is None and comment is not None:
            parts.append(f"    {comment}")
            continue

        # A comma is needed after the last code instruction if data follows
        needs_comma = not is_last_instr or bool(data_objects)
        quoted = f'    "{instr}"'
        if needs_comma:
            quoted += ","
        if comment:
            quoted += f" {comment}"
        parts.append(quoted)

    # ---- Data sections ----
    for oi, obj in enumerate(data_objects):
        num   = label_map[obj.name]
        words = pack_to_words(obj.entries)
        pow2  = p2align_power(obj.align)

        parts.append(f"    // --- rodata: {obj.name} ({len(obj.entries)} entries, {len(words)} words) ---")
        # parts.append(f'    ".section {obj.section}",') # don't emit this, we want the data in-lined
        parts.append(f'    ".p2align {pow2}",')
        parts.append(f'    "{num}:", // {obj.name}')

        is_last_obj = (oi == len(data_objects) - 1)
        for wi, w in enumerate(words):
            is_last_word = (wi == len(words) - 1)
            is_very_last = is_last_obj and is_last_word
            comma = "" if is_very_last else ","
            # Annotate with the original values that went into this word
            byte_offset = wi * 4
            n_entries_in_word = 4 // (obj.entries[0][0] if obj.entries else 4)
            entry_start = wi * n_entries_in_word
            entry_end   = min(entry_start + n_entries_in_word, len(obj.entries))
            vals = ", ".join(str(obj.entries[j][1]) for j in range(entry_start, entry_end))
            parts.append(f'    ".word 0x{w:08x}"{comma} // [{byte_offset}] {vals}')

    parts.append(");")
    return "\n".join(parts)


# ============================================================================
# Main
# ============================================================================

def main():
    args = parse_args()

    input_path, output_path, fn_name, start_label, end_label = resolve_paths(args)

    if not os.path.exists(input_path):
        print(
            f"Error: assembly file not found: {input_path}\n"
            f"  Run: python3 -m ziglang build dis -Dmodule={args.module}",
            file=sys.stderr,
        )
        sys.exit(1)

    out_dir = os.path.dirname(output_path)
    if out_dir and not os.path.isdir(out_dir):
        print(
            f"Error: output directory does not exist: {out_dir}\n"
            f"  Create the module subdirectory first.",
            file=sys.stderr,
        )
        sys.exit(1)

    with open(input_path, "r") as f:
        lines = f.readlines()

    # Pass 1a: extract code
    code_lines = extract_all_code(lines)
    if not code_lines:
        print(f"Error: no code found in {input_path}", file=sys.stderr)
        sys.exit(1)

    # Pass 1b: extract rodata / bss / data, splitting at .set alias offsets
    data_objects, alias_label_map = extract_rodata(lines)

    # Build unified label map
    code_label_positions = collect_all_labels(code_lines)
    label_map = build_label_map(code_label_positions, data_objects, args.label_base)

    # Sentinel positions for data labels: they are always forward from code
    all_label_positions = dict(code_label_positions)
    for obj in data_objects:
        all_label_positions[obj.name] = len(code_lines)

    # Diagnostics
    data_names = {o.name for o in data_objects}
    print("Label mapping:", file=sys.stderr)
    for name, num in sorted(label_map.items(), key=lambda x: x[1]):
        pos  = all_label_positions[name]
        kind = " (data)" if name in data_names else ""
        print(f"  {name:20s} -> {num}: (line {pos}){kind}", file=sys.stderr)

    if data_objects:
        print(f"\nData objects ({len(data_objects)}):", file=sys.stderr)
        for obj in data_objects:
            words = pack_to_words(obj.entries)
            print(f"  {obj.name}: {len(obj.entries)} entries -> {len(words)} words "
                  f"(section={obj.section}, align={obj.align})", file=sys.stderr)

    # Pass 2: convert code lines
    converted = []
    for i, line in enumerate(code_lines):
        instr, comment = convert_line(line, label_map, all_label_positions, i, alias_label_map)
        if instr is not None or comment is not None:
            converted.append((instr, comment))

    while converted and converted[-1] == (None, None):
        converted.pop()

    # Pass 3: apply coprocessor errata patches
    converted, errata_count = apply_errata_patches(converted)

    result = format_output(fn_name, start_label, end_label, converted, data_objects, label_map)

    with open(output_path, "w") as f:
        f.write(result + "\n")

    print(f"\nWrote {output_path}", file=sys.stderr)
    print(f"  input:  {input_path}", file=sys.stderr)
    print(f"  fn:     {fn_name}()", file=sys.stderr)
    print(f"  labels: {start_label} / {end_label}", file=sys.stderr)
    num_instrs = sum(1 for instr, _ in converted if instr is not None)
    print(f"  instructions: {num_instrs}", file=sys.stderr)
    func_labels = [n for n in label_map if not n.startswith('.') and n not in data_names]
    print(f"  functions found: {len(func_labels)} ({', '.join(func_labels)})", file=sys.stderr)
    if errata_count:
        print(f"  errata patches:  {errata_count}", file=sys.stderr)
    if len(WARNINGS) > 0:
        for warning in WARNINGS:
            print(warning)


if __name__ == "__main__":
    main()