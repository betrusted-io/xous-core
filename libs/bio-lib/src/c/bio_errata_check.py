
"""Post-link verification of BIO executable instructions (RV32IMC)."""

from pathlib import Path

# Documented BIO phantom-register errata.
BUG1_OPCODES = {0x17, 0x37, 0x6F}  # AUIPC, LUI, JAL
BUG2_OPCODES = {
    0x03, 0x0F, 0x13, 0x17, 0x37, 0x67, 0x6F, 0x73
}  # Non-R/S/B opcodes in RV32IMC


def scan_linked_code(image, code_end):
    """Scan executable bytes, excluding appended data and alignment."""
    if not (0 < code_end <= len(image)) or code_end % 2:
        raise ValueError(
            f"Invalid executable extent 0x{code_end:x} "
            f"for {len(image)}-byte image"
        )

    issues = []
    pc = 0

    while pc < code_end:
        halfword = int.from_bytes(image[pc:pc + 2], "little")

        if (halfword & 0x3) != 0x3:
            pc += 2
            continue

        if (halfword & 0x1F) == 0x1F:
            raise ValueError(
                f"Unsupported >=48-bit instruction at 0x{pc:04x}"
            )

        if pc + 4 > code_end:
            raise ValueError(
                f"Truncated 32-bit instruction at 0x{pc:04x}"
            )

        word = int.from_bytes(image[pc:pc + 4], "little")
        opcode = word & 0x7F

        if opcode in BUG1_OPCODES and ((word >> 15) & 31) in range(16, 20):
            issues.append((pc, word, "BUG1 phantom-rs1"))

        if opcode in BUG2_OPCODES and ((word >> 20) & 31) == 20:
            issues.append((pc, word, "BUG2 phantom-rs2"))

        pc += 4

    return issues


def check_linked_binary(bin_path):
    """Validate and strip a 4-byte linker-resolved code-end footer."""
    path = Path(bin_path)
    linked = path.read_bytes()

    if len(linked) < 6:
        raise ValueError("Missing or truncated BIO code-end footer")

    code_end = int.from_bytes(linked[-4:], "little")
    image = linked[:-4]

    if len(image) > 0xF00:
        raise ValueError(
            f"BIO image exceeds 0xF00 bytes: {len(image)} bytes"
        )

    issues = scan_linked_code(image, code_end)

    if issues:
        report = ", ".join(
            f"0x{pc:04x}=0x{word:08x} ({kind})"
            for pc, word, kind in issues
        )
        raise ValueError(f"Unsafe linked BIO instructions: {report}")

    # Write only the actual executable image and its data.
    path.write_bytes(image)
    return code_end
