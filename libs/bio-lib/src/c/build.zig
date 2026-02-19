// build.zig
const std = @import("std");

pub fn build(b: *std.Build) void {
    const optimize = b.standardOptimizeOption(.{});

    const linker_script = b.option(
        []const u8,
        "linker",
        "Path to linker script (optional)",
    );

    // Module name for the "dis" step: e.g. -Dmodule=math_test
    // Defaults to "test" to match the legacy layout.
    const module_name = b.option(
        []const u8,
        "module",
        "Subdirectory module to compile (e.g. math_test). " ++
            "Expects <module>/main.c to exist.",
    ) orelse "test";

    // -- RV32IMC target with x16-x31 reserved -------------------------
    //
    // The reserve_x* features tell LLVM's register allocator to never
    // use these registers. This is the reliable way to reserve registers
    // -- the -ffixed-xN C flags depend on the clang driver translating
    // them into +reserve_xN target features, which zig cc may not do
    // correctly when invoked as a system command.
    const target = b.resolveTargetQuery(.{
        .cpu_arch = .riscv32,
        .os_tag = .freestanding,
        .abi = .none,
        .cpu_model = .{ .explicit = &std.Target.riscv.cpu.generic_rv32 },
        .cpu_features_add = blk: {
            var features = std.Target.Cpu.Feature.Set.empty;
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.m));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.c));
            // Reserve x16-x31 for coprocessor use (FIFOs, special regs).
            // This prevents the compiler from using these registers for
            // general-purpose allocation, which would conflict with the
            // inline asm in bio.h that reads/writes them directly.
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x16));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x17));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x18));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x19));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x20));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x21));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x22));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x23));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x24));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x25));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x26));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x27));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x28));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x29));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x30));
            features.addFeature(@intFromEnum(std.Target.riscv.Feature.reserve_x31));
            break :blk features;
        },
    });

    // -- Build the firmware ELF (uses the selected module) ------------
    const exe = b.addExecutable(.{
        .name = "firmware",
        .root_module = b.createModule(.{
            .target = target,
            .optimize = optimize,
        }),
    });

    // main.c lives inside the module subdirectory
    const main_c_path = b.pathJoin(&.{ module_name, "main.c" });
    exe.root_module.addCSourceFile(.{
        .file = b.path(main_c_path),
        .flags = c_flags,
    });

    // Headers: module dir first (bio.h, fp.h, etc.), then top-level include/
    exe.root_module.addIncludePath(b.path(module_name));
    exe.root_module.addIncludePath(b.path("include"));

    // -- Linker script ------------------------------------------------
    if (linker_script) |ld| {
        exe.setLinkerScript(b.path(ld));
    }

    b.installArtifact(exe);

    // -- "zig build dis -Dmodule=<n>" - emit assembly -----------------
    // Output: zig-out/<module_name>.s
    //
    // This invokes "zig cc -S" as a separate system command, so it does
    // NOT inherit the target features from the resolveTargetQuery above.
    // We must pass the reserve features explicitly via -mcpu.
    const dis_step = b.step("dis", "Generate assembly from C source");

    const dis_cmd = b.addSystemCommand(&.{
        b.graph.zig_exe,
        "cc",
        "-S", // stop after compilation, emit assembly
        "-fverbose-asm",
        "-o",
    });
    dis_cmd.has_side_effects = true;

    // The output filename is the module name with a .s extension.
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
    // Include paths for the compiler invocation
    dis_cmd.addArg(b.fmt("-I{s}", .{module_name}));
    dis_cmd.addArg("-Iinclude");
    dis_cmd.addArgs(c_flags);

    // Install the .s file into zig-out/ at the top level.
    const install_dis = b.addInstallFile(dis_output, out_s_name);
    dis_step.dependOn(&install_dis.step);
}

// ---------------------------------------------------------------------
// C compiler flags shared by both the ELF build and the dis step.
//
// Note: register reservation is NOT done here via -ffixed-xN because
// zig cc does not reliably translate those into LLVM +reserve_xN
// features. Instead, reservation is handled via:
//   - cpu_features_add in the target query (ELF build)
//   - the -mcpu string (dis step)
// The -ffixed-xN flags are kept as a belt-and-suspenders measure in
// case a future zig version starts honoring them, but they are not
// the primary mechanism.
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
