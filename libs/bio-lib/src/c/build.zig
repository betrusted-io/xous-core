// build.zig
const std = @import("std");

pub fn build(b: *std.Build) void {
    // -Dmodule=<n>: subdirectory to compile. Expects <n>/main.c to exist.
    const module_name = b.option(
        []const u8,
        "module",
        "Subdirectory module to compile (e.g. math_test). Expects <module>/main.c to exist.",
    ) orelse "test";

    // -Dasm-only=true: emit .s file but skip running clang2rustasm.py
    const asm_only = b.option(
        bool,
        "asm-only",
        "Stop after emitting .s; do not run clang2rustasm.py",
    ) orelse false;

    const main_c_path = b.pathJoin(&.{ module_name, "main.c" });

    // -- Step 1: emit assembly ----------------------------------------
    //
    // Invokes "zig cc -S" as a separate system command. Target features
    // (RV32IMC + x16-x31 reserved) are passed explicitly via -mcpu since
    // we are driving the compiler directly rather than via addExecutable.
    const dis_cmd = b.addSystemCommand(&.{
        b.graph.zig_exe,
        "cc",
        "-S",
        "-fverbose-asm",
        "-o",
    });
    dis_cmd.has_side_effects = true;

    const out_s_name = b.fmt("{s}.s", .{module_name});
    const dis_output = dis_cmd.addOutputFileArg(out_s_name);

    dis_cmd.addFileArg(b.path(main_c_path));
    dis_cmd.addArgs(&.{
        "-target",
        "riscv32-freestanding",
        // Pack the reserve features directly into the -mcpu string.
        // This bypasses any issues with -ffixed-xN not being translated
        // into LLVM target features by the zig cc driver.
        "-mcpu=generic_rv32+m+c" ++
            "+reserve_x16+reserve_x17+reserve_x18+reserve_x19" ++
            "+reserve_x20+reserve_x21+reserve_x22+reserve_x23" ++
            "+reserve_x24+reserve_x25+reserve_x26+reserve_x27" ++
            "+reserve_x28+reserve_x29+reserve_x30+reserve_x31",
    });
    dis_cmd.addArg(b.fmt("-I{s}", .{module_name}));
    dis_cmd.addArg("-Iinclude");
    dis_cmd.addArgs(c_flags);

    const install_dis = b.addInstallFile(dis_output, out_s_name);
    install_dis.step.dependOn(&dis_cmd.step);

    // -- Step 2: convert assembly to Rust inline asm ------------------
    //
    // Runs: python3 clang2rustasm.py <module_name>
    // The script lives next to build.zig. Skipped when -Dasm-only=true.
    const py_cmd = b.addSystemCommand(&.{
        "python3",
        b.pathFromRoot("clang2rustasm.py"),
        module_name,
    });
    py_cmd.has_side_effects = true;
    py_cmd.step.dependOn(&install_dis.step);

    // -- Default step: full pipeline or asm-only ----------------------
    //
    // Plain "zig build -Dmodule=math_test" runs the full pipeline.
    // Add "-Dasm-only=true" to stop after the .s file.
    if (asm_only) {
        b.default_step.dependOn(&install_dis.step);
    } else {
        b.default_step.dependOn(&py_cmd.step);
    }
}

// ---------------------------------------------------------------------
// C compiler flags for the dis step.
//
// Note: register reservation is primarily handled via the -mcpu string
// (+reserve_xN features) passed to zig cc above. The -ffixed-xN flags
// below are belt-and-suspenders in case a future zig version starts
// translating them into LLVM target features, but they are not the
// primary mechanism and may be silently ignored today.
// ---------------------------------------------------------------------
const c_flags = &[_][]const u8{
    "-std=c11",
    "-Wall",
    "-Wextra",
    "-ffreestanding",
    "-fno-common",
    "-ffunction-sections",
    "-fdata-sections",
    "-Os",

    // Belt-and-suspenders: these may or may not be honored by zig cc,
    // but the real reservation is done via target features (see above).
    "-ffixed-x16",
    "-ffixed-x17",
    "-ffixed-x18",
    "-ffixed-x19",
    "-ffixed-x20",
    "-ffixed-x21",
    "-ffixed-x22",
    "-ffixed-x23",
    "-ffixed-x24",
    "-ffixed-x25",
    "-ffixed-x26",
    "-ffixed-x27",
    "-ffixed-x28",
    "-ffixed-x29",
    "-ffixed-x30",
    "-ffixed-x31",
};
