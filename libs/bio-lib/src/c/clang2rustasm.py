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


def extract_rodata(lines):
    """
    Walk the file and collect all data objects from .rodata* sections.
    Returns list of DataObject in source order.
    """
    objects = []
    in_rodata = False
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

        # Section transitions
        if stripped.startswith(".section"):
            if current_obj is not None:
                objects.append(current_obj)
                current_obj = None

            if ".rodata" in stripped:
                in_rodata = True
                m = re.match(r'\.section\s+([^\s,]+)', stripped)
                current_section = m.group(1) if m else ".rodata"
                pending_align = 1
            else:
                in_rodata = False
                current_section = None
            continue

        if not in_rodata:
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

        # Object label: starts a new DataObject
        obj_label = re.match(r'^([a-zA-Z_]\w*)\s*:', stripped)
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

    return objects


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

def convert_line(line, label_map, label_positions, current_line_idx):
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
    # Data labels are always forward refs; code labels use position comparison.
    def replace_reloc(m):
        kind  = m.group(1)
        lname = m.group(2)
        if lname in label_map:
            num  = label_map[lname]
            lpos = label_positions.get(lname, current_line_idx + 1)
            ref  = f"{num}f" if lpos > current_line_idx else f"{num}b"
            return f"%{kind}({ref})"
        return m.group(0)

    instr = re.sub(r'%(\w+)\(([a-zA-Z_]\w*)\)', replace_reloc, instr)

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
    parts.append("use bao1x_api::bio_code;\n")
    parts.append("#[rustfmt::skip]")
    parts.append(f"bio_code!({fn_name}, {start_label}, {end_label},")

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

    # Pass 1b: extract rodata
    data_objects = extract_rodata(lines)

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
        instr, comment = convert_line(line, label_map, all_label_positions, i)
        if instr is not None or comment is not None:
            converted.append((instr, comment))

    while converted and converted[-1] == (None, None):
        converted.pop()

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


if __name__ == "__main__":
    main()