pub type RvInst = u64;

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum RvIsa {
    Rv32 = 0,
    Rv64 = 1,
    Rv128 = 2,
}

pub type RwFenceType = u8;
pub const RV_FENCE_W: RwFenceType = 1;
pub const RV_FENCE_R: RwFenceType = 2;
pub const RV_FENCE_O: RwFenceType = 4;
pub const RV_FENCE_I: RwFenceType = 8;

pub const RV_IREG_SP: u8 = 2;
pub const RV_IREG_RA: u8 = 1;
pub const RV_IREG_ZERO: u8 = 0;

pub type RvcConstraint = u32;
pub const RVC_CSR_EQ_0XC82: RvcConstraint = 18;
pub const RVC_CSR_EQ_0XC81: RvcConstraint = 17;
pub const RVC_CSR_EQ_0XC80: RvcConstraint = 16;
pub const RVC_CSR_EQ_0XC02: RvcConstraint = 15;
pub const RVC_CSR_EQ_0XC01: RvcConstraint = 14;
pub const RVC_CSR_EQ_0XC00: RvcConstraint = 13;
pub const RVC_CSR_EQ_0X003: RvcConstraint = 12;
pub const RVC_CSR_EQ_0X002: RvcConstraint = 11;
pub const RVC_CSR_EQ_0X001: RvcConstraint = 10;
pub const RVC_IMM_EQ_P1: RvcConstraint = 9;
pub const RVC_IMM_EQ_N1: RvcConstraint = 8;
pub const RVC_IMM_EQ_ZERO: RvcConstraint = 7;
pub const RVC_IMM_EQ_RA: RvcConstraint = 6;
pub const RVC_RS2_EQ_RS1: RvcConstraint = 5;
pub const RVC_RS2_EQ_X0: RvcConstraint = 4;
pub const RVC_RS1_EQ_X0: RvcConstraint = 3;
pub const RVC_RD_EQ_X0: RvcConstraint = 2;
pub const RVC_RD_EQ_RA: RvcConstraint = 1;

pub type RvCodec = u8;
pub const RV_CODEC_RSS_SQSP: RvCodec = 47;
pub const RV_CODEC_CSS_SDSP: RvCodec = 46;
pub const RV_CODEC_CSS_SWSP: RvCodec = 45;
pub const RV_CODEC_CS_SQ: RvCodec = 44;
pub const RV_CODEC_CS_SD: RvCodec = 43;
pub const RV_CODEC_CS_SW: RvCodec = 42;
pub const RV_CODEC_CS: RvCodec = 41;
pub const RV_CODEC_CR_JR: RvCodec = 40;
pub const RV_CODEC_CR_JALR: RvCodec = 39;
pub const RV_CODEC_CR_MV: RvCodec = 38;
pub const RV_CODEC_CR: RvCodec = 37;
pub const RV_CODEC_CL_LQ: RvCodec = 36;
pub const RV_CODEC_CL_LD: RvCodec = 35;
pub const RV_CODEC_CL_LW: RvCodec = 34;
pub const RV_CODEC_CJ_JAL: RvCodec = 33;
pub const RV_CODEC_CJ: RvCodec = 32;
pub const RV_CODEC_CIW_4SPN: RvCodec = 31;
pub const RV_CODEC_CI_NONE: RvCodec = 30;
pub const RV_CODEC_CI_LUI: RvCodec = 29;
pub const RV_CODEC_CI_LI: RvCodec = 28;
pub const RV_CODEC_CI_LQSP: RvCodec = 27;
pub const RV_CODEC_CI_LDSP: RvCodec = 26;
pub const RV_CODEC_CI_LWSP: RvCodec = 25;
pub const RV_CODEC_CI_16SP: RvCodec = 24;
pub const RV_CODEC_CI_SH6: RvCodec = 23;
pub const RV_CODEC_CI: RvCodec = 21;
pub const RV_CODEC_CB_SH6: RvCodec = 20;
pub const RV_CODEC_CB_IMM: RvCodec = 18;
pub const RV_CODEC_CB: RvCodec = 17;
pub const RV_CODEC_R_F: RvCodec = 16;
pub const RV_CODEC_R_L: RvCodec = 15;
pub const RV_CODEC_R_A: RvCodec = 14;
pub const RV_CODEC_R4_M: RvCodec = 13;
pub const RV_CODEC_R_M: RvCodec = 12;
pub const RV_CODEC_R: RvCodec = 11;
pub const RV_CODEC_SB: RvCodec = 10;
pub const RV_CODEC_S: RvCodec = 9;
pub const RV_CODEC_I_CSR: RvCodec = 8;
pub const RV_CODEC_I_SH7: RvCodec = 7;
pub const RV_CODEC_I_SH6: RvCodec = 6;
pub const RV_CODEC_I_SH5: RvCodec = 5;
pub const RV_CODEC_I: RvCodec = 4;
pub const RV_CODEC_UJ: RvCodec = 3;
pub const RV_CODEC_U: RvCodec = 2;
pub const RV_CODEC_NONE: RvCodec = 1;
pub const RV_CODEC_ILLEGAL: RvCodec = 0;

#[derive(Clone, Copy)]
#[repr(usize)]
enum RvOp {
    Illegal,
    Lui,
    Auipc,
    Jal,
    Jalr,
    Beq,
    Bne,
    Blt,
    Bge,
    Bltu,
    Bgeu,
    Lb,
    Lh,
    Lw,
    Lbu,
    Lhu,
    Sb,
    Sh,
    Sw,
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
    Slli,
    Srli,
    Srai,
    Add,
    Sub,
    Sll,
    Slt,
    Sltu,
    Xor,
    Srl,
    Sra,
    Or,
    And,
    Fence,
    FenceI,
    Lwu,
    Ld,
    Sd,
    Addiw,
    Slliw,
    Srliw,
    Sraiw,
    Addw,
    Subw,
    Sllw,
    Srlw,
    Sraw,
    Ldu,
    Lq,
    Sq,
    Addid,
    Sllid,
    Srlid,
    Sraid,
    Addd,
    Subd,
    Slld,
    Srld,
    Srad,
    Mul,
    Mulh,
    Mulhsu,
    Mulhu,
    Div,
    Divu,
    Rem,
    Remu,
    Mulw,
    Divw,
    Divuw,
    Remw,
    Remuw,
    Muld,
    Divd,
    Divud,
    Remd,
    Remud,
    LrW,
    ScW,
    AmoswapW,
    AmoaddW,
    AmoxorW,
    AmoorW,
    AmoandW,
    AmominW,
    AmomaxW,
    AmominuW,
    AmomaxuW,
    LrD,
    ScD,
    AmoswapD,
    AmoaddD,
    AmoxorD,
    AmoorD,
    AmoandD,
    AmominD,
    AmomaxD,
    AmominuD,
    AmomaxuD,
    LrQ,
    ScQ,
    AmoswapQ,
    AmoaddQ,
    AmoxorQ,
    AmoorQ,
    AmoandQ,
    AmominQ,
    AmomaxQ,
    AmominuQ,
    AmomaxuQ,
    Ecall,
    Ebreak,
    Uret,
    Sret,
    Hret,
    Mret,
    Dret,
    SfenceVm,
    SfenceVma,
    Wfi,
    Csrrw,
    Csrrs,
    Csrrc,
    Csrrwi,
    Csrrsi,
    Csrrci,
    Flw,
    Fsw,
    FmaddS,
    FmsubS,
    FnmsubS,
    FnmaddS,
    FaddS,
    FsubS,
    FmulS,
    FdivS,
    FsgnjS,
    FsgnjnS,
    FsgnjxS,
    FminS,
    FmaxS,
    FsqrtS,
    FleS,
    FltS,
    FeqS,
    FcvtWS,
    FcvtWuS,
    FcvtSW,
    FcvtSWu,
    FmvXS,
    FclassS,
    FmvSX,
    FcvtLS,
    FcvtLuS,
    FcvtSL,
    FcvtSLu,
    Fld,
    Fsd,
    FmaddD,
    FmsubD,
    FnmsubD,
    FnmaddD,
    FaddD,
    FsubD,
    FmulD,
    FdivD,
    FsgnjD,
    FsgnjnD,
    FsgnjxD,
    FminD,
    FmaxD,
    FcvtSD,
    FcvtDS,
    FsqrtD,
    FleD,
    FltD,
    FeqD,
    FcvtWD,
    FcvtWuD,
    FcvtDW,
    FcvtDWu,
    FclassD,
    FcvtLD,
    FcvtLuD,
    FmvXD,
    FcvtDL,
    FcvtDLu,
    FmvDX,
    Flq,
    Fsq,
    FmaddQ,
    FmsubQ,
    FnmsubQ,
    FnmaddQ,
    FaddQ,
    FsubQ,
    FmulQ,
    FdivQ,
    FsgnjQ,
    FsgnjnQ,
    FsgnjxQ,
    FminQ,
    FmaxQ,
    FcvtSQ,
    FcvtQS,
    FcvtDQ,
    FcvtQD,
    FsqrtQ,
    FleQ,
    FltQ,
    FeqQ,
    FcvtWQ,
    FcvtWuQ,
    FcvtQW,
    FcvtQWu,
    FclassQ,
    FcvtLQ,
    FcvtLuQ,
    FcvtQL,
    FcvtQLu,
    FmvXQ,
    FmvQX,
    CAddi4spn,
    CFld,
    CLw,
    CFlw,
    CFsd,
    CSw,
    CFsw,
    CNop,
    CAddi,
    CJal,
    CLi,
    CAddi16sp,
    CLui,
    CSrli,
    CSrai,
    CAndi,
    CSub,
    CXor,
    COr,
    CAnd,
    CSubw,
    CAddw,
    CJ,
    CBeqz,
    CBnez,
    CSlli,
    CFldsp,
    CLwsp,
    CFlwsp,
    CJr,
    CMv,
    CEbreak,
    CJalr,
    CAdd,
    CFsdsp,
    CSwsp,
    CFswsp,
    CLd,
    CSd,
    CAddiw,
    CLdsp,
    CSdsp,
    CLq,
    CSq,
    CLqsp,
    CSqsp,
    Nop,
    Mv,
    Not,
    Neg,
    Negw,
    SextW,
    Seqz,
    Snez,
    Sltz,
    Sgtz,
    FmvS,
    FabsS,
    FnegS,
    FmvD,
    FabsD,
    FnegD,
    FmvQ,
    FabsQ,
    FnegQ,
    Beqz,
    Bnez,
    Blez,
    Bgez,
    Bltz,
    Bgtz,
    Ble,
    Bleu,
    Bgt,
    Bgtu,
    J,
    Ret,
    Jr,
    Rdcycle,
    Rdtime,
    Rdinstret,
    Rdcycleh,
    Rdtimeh,
    Rdinstreth,
    Frcsr,
    Frrm,
    Frflags,
    Fscsr,
    Fsrm,
    Fsflags,
    Fsrmi,
    Fsflagsi,
}

impl Default for RvOp {
    fn default() -> Self {
        Self::Ldu
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct RvDecode {
    pc: u64,
    inst: u64,
    imm: i32,
    op: RvOp,
    codec: u8,
    rd: u8,
    rs1: u8,
    rs2: u8,
    rs3: u8,
    rm: u8,
    pred: u8,
    succ: u8,
    aq: u8,
    rl: u8,
}

#[derive(Copy, Clone)]
pub struct RvOpcodeData {
    name: &'static str,
    codec: RvCodec,
    format: &'static [u8],
    pseudo: &'static [RvCompData],
    decomp_rv32: Option<RvOp>,
    decomp_rv64: Option<RvOp>,
    decomp_rv128: Option<RvOp>,
    decomp_data: RvCdImmediate,
}

#[derive(Copy, Clone)]
pub struct RvCompData {
    op: RvOp,
    constraints: &'static [RvcConstraint],
}

#[derive(PartialEq, Copy, Clone)]
enum RvCdImmediate {
    None,
    Nz,
    NzHint,
}

const RV_IREG_NAME_SYM: &[&str] = &[
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

const RV_FREG_NAME_SYM: &[&str] = &[
    "ft0", "ft1", "ft2", "ft3", "ft4", "ft5", "ft6", "ft7", "fs0", "fs1", "fa0", "fa1", "fa2",
    "fa3", "fa4", "fa5", "fa6", "fa7", "fs2", "fs3", "fs4", "fs5", "fs6", "fs7", "fs8", "fs9",
    "fs10", "fs11", "ft8", "ft9", "ft10", "ft11",
];

const RV_FMT_NONE: &[u8] = b"O\t";
const RV_FMT_RS1: &[u8] = b"O\t1";
const RV_FMT_OFFSET: &[u8] = b"O\to";
const RV_FMT_PRED_SUCC: &[u8] = b"O\tp,s";
const RV_FMT_RS1_RS2: &[u8] = b"O\t1,2";
const RV_FMT_RD_IMM: &[u8] = b"O\t0,i";
const RD_FMT_RD_OFFSET: &[u8] = b"O\t0,o";
const RV_FMT_RD_RS1_RS2: &[u8] = b"O\t0,1,2";
const RV_FMT_FRD_RS1: &[u8] = b"O\t3,1";
const RV_FMT_RD_FRS1: &[u8] = b"O\t0,4";
const RV_FMT_RD_FRS1_FRS2: &[u8] = b"O\t0,4,5";
const RV_FMT_FRD_FRS1_FRS2: &[u8] = b"O\t3,4,5";
const RV_FMT_RM_FRD_FRS1: &[u8] = b"O\tr,3,4";
const RV_FMT_RM_FRD_RS1: &[u8] = b"O\tr,3,1";
const RV_FMT_RM_RD_FRS1: &[u8] = b"O\tr,0,4";
const RV_FMT_RM_FRD_FRS1_FRS2: &[u8] = b"O\tr,3,4,5";
const RV_FMT_RM_FRD_FRS1_FRS2_FRS3: &[u8] = b"O\tr,3,4,5,6";
const RV_FMT_RD_RS1_IMM: &[u8] = b"O\t0,1,i";
const RV_FMT_RD_RS1_OFFSET: &[u8] = b"O\t0,1,i";
const RV_FMT_RD_OFFSET_RS1: &[u8] = b"O\t0,i(1)";
const RV_FMT_FRD_OFFSET_RS1: &[u8] = b"O\t3,i(1)";
const RV_FMT_RD_CSR_RS1: &[u8] = b"O\t0,c,1";
const RV_FMT_RD_CSR_ZIMM: &[u8] = b"O\t0,c,7";
const RV_FMT_RS2_OFFSET_RS1: &[u8] = b"O\t2,i(1)";
const RV_FMT_FRS2_OFFSET_RS1: &[u8] = b"O\t5,i(1)";
const RV_FMT_RS1_RS2_OFFSET: &[u8] = b"O\t1,2,o";
const RV_FMT_RS2_RS1_OFFSET: &[u8] = b"O\t2,1,o";
const RV_FMT_AQRL_RD_RS2_RS1: &[u8] = b"OAR\t0,2,(1)";
const RV_FMT_AQRL_RD_RS1: &[u8] = b"OAR\t0,(1)";
const RV_FMT_RD: &[u8] = b"O\t0";
const RV_FMT_RD_ZIMM: &[u8] = b"O\t0,7";
const RV_FMT_RD_RS1: &[u8] = b"O\t0,1";
const RV_FMT_RD_RS2: &[u8] = b"O\t0,2";
const RV_FMT_RS1_OFFSET: &[u8] = b"O\t1,o";
const RV_FMT_RS2_OFFSET: &[u8] = b"O\t2,o";

const RVCC_IMM_EQ_ZERO: [RvcConstraint; 1] = [RVC_IMM_EQ_ZERO];
const RVCC_IMM_EQ_N1: [RvcConstraint; 1] = [RVC_IMM_EQ_N1];
const RVCC_IMM_EQ_P1: [RvcConstraint; 1] = [RVC_IMM_EQ_P1];
const RVCC_RS1_EQ_X0: [RvcConstraint; 1] = [RVC_RS1_EQ_X0];
const RVCC_RS2_EQ_X0: [RvcConstraint; 1] = [RVC_RS2_EQ_X0];
const RVCC_RS2_EQ_RS1: [RvcConstraint; 1] = [RVC_RS2_EQ_RS1];
const RVCC_JAL_J: [RvcConstraint; 1] = [RVC_RD_EQ_X0];
const RVCC_JAL_JAL: [RvcConstraint; 1] = [RVC_RD_EQ_RA];
const RVCC_JALR_JR: [RvcConstraint; 2] = [RVC_RD_EQ_X0, RVC_IMM_EQ_ZERO];
const RVCC_JALR_JALR: [RvcConstraint; 2] = [RVC_RD_EQ_RA, RVC_IMM_EQ_ZERO];
const RVCC_JALR_RET: [RvcConstraint; 2] = [RVC_RD_EQ_X0, RVC_IMM_EQ_RA];
const RVCC_ADDI_NOP: [RvcConstraint; 3] = [RVC_RD_EQ_X0, RVC_RS1_EQ_X0, RVC_IMM_EQ_ZERO];
const RVCC_RDCYCLE: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC00];
const RVCC_RDTIME: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC01];
const RVCC_RDINSTRET: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC02];
const RVCC_RDCYCLEH: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC80];
const RVCC_RDTIMEH: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC81];
const RVCC_RDINSTRETH: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0XC82];
const RVCC_FRCSR: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0X003];
const RVCC_FRRM: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0X002];
const RVCC_FRFLAGS: [RvcConstraint; 2] = [RVC_RS1_EQ_X0, RVC_CSR_EQ_0X001];
const FVCC_FSCSR: [RvcConstraint; 1] = [RVC_CSR_EQ_0X003];
const RVCC_FSRM: [RvcConstraint; 1] = [RVC_CSR_EQ_0X002];
const RVCC_FSFLAGS: [RvcConstraint; 1] = [RVC_CSR_EQ_0X001];
const RVCC_FSRMI: [RvcConstraint; 1] = [RVC_CSR_EQ_0X002];
const RVCC_FSFLAGSI: [RvcConstraint; 1] = [RVC_CSR_EQ_0X001];

const RVCP_JAL: [RvCompData; 2] = [
    RvCompData {
        op: RvOp::J,
        constraints: &RVCC_JAL_J,
    },
    RvCompData {
        op: RvOp::Jal,
        constraints: &RVCC_JAL_JAL,
    },
];

const RVCP_JALR: [RvCompData; 3] = [
    RvCompData {
        op: RvOp::Ret,
        constraints: &RVCC_JALR_RET,
    },
    RvCompData {
        op: RvOp::Jr,
        constraints: &RVCC_JALR_JR,
    },
    RvCompData {
        op: RvOp::Jalr,
        constraints: &RVCC_JALR_JALR,
    },
];

const RVCP_BEQ: [RvCompData; 1] = [RvCompData {
    op: RvOp::Beqz,
    constraints: &RVCC_RS2_EQ_X0,
}];

const RVCP_BNE: [RvCompData; 1] = [RvCompData {
    op: RvOp::Bnez,
    constraints: &RVCC_RS2_EQ_X0,
}];

const RVCP_BLT: [RvCompData; 3] = [
    RvCompData {
        op: RvOp::Bltz,
        constraints: &RVCC_RS2_EQ_X0,
    },
    RvCompData {
        op: RvOp::Bgtz,
        constraints: &RVCC_RS1_EQ_X0,
    },
    RvCompData {
        op: RvOp::Bgt,
        constraints: &[],
    },
];

const RVCP_BGE: [RvCompData; 3] = [
    RvCompData {
        op: RvOp::Blez,
        constraints: &RVCC_RS1_EQ_X0,
    },
    RvCompData {
        op: RvOp::Bgez,
        constraints: &RVCC_RS2_EQ_X0,
    },
    RvCompData {
        op: RvOp::Ble,
        constraints: &[],
    },
];

const RVCP_BLTU: [RvCompData; 1] = [RvCompData {
    op: RvOp::Bgtu,
    constraints: &[],
}];

const RVCP_BGEU: [RvCompData; 1] = [RvCompData {
    op: RvOp::Bleu,
    constraints: &[],
}];

const RVCP_ADDI: [RvCompData; 2] = [
    RvCompData {
        op: RvOp::Nop,
        constraints: &RVCC_ADDI_NOP,
    },
    RvCompData {
        op: RvOp::Mv,
        constraints: &RVCC_IMM_EQ_ZERO,
    },
];

const RVCP_SLTIU: [RvCompData; 1] = [RvCompData {
    op: RvOp::Seqz,
    constraints: &RVCC_IMM_EQ_P1,
}];

const RVCP_XORI: [RvCompData; 1] = [RvCompData {
    op: RvOp::Not,
    constraints: &RVCC_IMM_EQ_N1,
}];

const RVCP_SUB: [RvCompData; 1] = [RvCompData {
    op: RvOp::Neg,
    constraints: &RVCC_RS1_EQ_X0,
}];

const RVCP_SLT: [RvCompData; 2] = [
    RvCompData {
        op: RvOp::Sltz,
        constraints: &RVCC_RS2_EQ_X0,
    },
    RvCompData {
        op: RvOp::Sgtz,
        constraints: &RVCC_RS1_EQ_X0,
    },
];

const RVCP_SLTU: [RvCompData; 1] = [RvCompData {
    op: RvOp::Snez,
    constraints: &RVCC_RS1_EQ_X0,
}];

const RVCP_ADDIW: [RvCompData; 1] = [RvCompData {
    op: RvOp::SextW,
    constraints: &RVCC_IMM_EQ_ZERO,
}];

const RVCP_SUBW: [RvCompData; 1] = [RvCompData {
    op: RvOp::Negw,
    constraints: &RVCC_RS1_EQ_X0,
}];

const RVCP_CSRRW: [RvCompData; 3] = [
    RvCompData {
        op: RvOp::Fscsr,
        constraints: &FVCC_FSCSR,
    },
    RvCompData {
        op: RvOp::Fsrm,
        constraints: &RVCC_FSRM,
    },
    RvCompData {
        op: RvOp::Fsflags,
        constraints: &RVCC_FSFLAGS,
    },
];

const RVCP_CSRRS: [RvCompData; 9] = [
    RvCompData {
        op: RvOp::Rdcycle,
        constraints: &RVCC_RDCYCLE,
    },
    RvCompData {
        op: RvOp::Rdtime,
        constraints: &RVCC_RDTIME,
    },
    RvCompData {
        op: RvOp::Rdinstret,
        constraints: &RVCC_RDINSTRET,
    },
    RvCompData {
        op: RvOp::Rdcycleh,
        constraints: &RVCC_RDCYCLEH,
    },
    RvCompData {
        op: RvOp::Rdtimeh,
        constraints: &RVCC_RDTIMEH,
    },
    RvCompData {
        op: RvOp::Rdinstreth,
        constraints: &RVCC_RDINSTRETH,
    },
    RvCompData {
        op: RvOp::Frcsr,
        constraints: &RVCC_FRCSR,
    },
    RvCompData {
        op: RvOp::Frrm,
        constraints: &RVCC_FRRM,
    },
    RvCompData {
        op: RvOp::Frflags,
        constraints: &RVCC_FRFLAGS,
    },
];

const RVCP_CSRRWI: [RvCompData; 2] = [
    RvCompData {
        op: RvOp::Fsrmi,
        constraints: &RVCC_FSRMI,
    },
    RvCompData {
        op: RvOp::Fsflagsi,
        constraints: &RVCC_FSFLAGSI,
    },
];

const RVCP_FSGNJ_S: [RvCompData; 1] = [RvCompData {
    op: RvOp::FmvS,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJN_S: [RvCompData; 1] = [RvCompData {
    op: RvOp::FnegS,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJX_S: [RvCompData; 1] = [RvCompData {
    op: RvOp::FabsS,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJ_D: [RvCompData; 1] = [RvCompData {
    op: RvOp::FmvD,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJN_D: [RvCompData; 1] = [RvCompData {
    op: RvOp::FnegD,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJX_D: [RvCompData; 1] = [RvCompData {
    op: RvOp::FabsD,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJ_Q: [RvCompData; 1] = [RvCompData {
    op: RvOp::FmvQ,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJN_Q: [RvCompData; 1] = [RvCompData {
    op: RvOp::FnegQ,
    constraints: &RVCC_RS2_EQ_RS1,
}];

const RVCP_FSGNJX_Q: [RvCompData; 1] = [RvCompData {
    op: RvOp::FabsQ,
    constraints: &RVCC_RS2_EQ_RS1,
}];

pub const OPCODE_DATA: [RvOpcodeData; 319] = [
    RvOpcodeData {
        name: "illegal",
        codec: RV_CODEC_ILLEGAL,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lui",
        codec: RV_CODEC_U,
        format: RV_FMT_RD_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "auipc",
        codec: RV_CODEC_U,
        format: RD_FMT_RD_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "jal",
        codec: RV_CODEC_UJ,
        format: RD_FMT_RD_OFFSET,
        pseudo: &RVCP_JAL,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "jalr",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_OFFSET,
        pseudo: &RVCP_JALR,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "beq",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BEQ,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bne",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BNE,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "blt",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BLT,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bge",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BGE,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bltu",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BLTU,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bgeu",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &RVCP_BGEU,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lb",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lh",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lw",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lbu",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lhu",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sb",
        codec: RV_CODEC_S,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sh",
        codec: RV_CODEC_S,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sw",
        codec: RV_CODEC_S,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "addi",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &RVCP_ADDI,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "slti",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sltiu",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &RVCP_SLTIU,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "xori",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &RVCP_XORI,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ori",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "andi",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "slli",
        codec: RV_CODEC_I_SH7,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srli",
        codec: RV_CODEC_I_SH7,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srai",
        codec: RV_CODEC_I_SH7,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "add",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sub",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &RVCP_SUB,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sll",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "slt",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &RVCP_SLT,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sltu",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &RVCP_SLTU,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "xor",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srl",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sra",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "or",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "and",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fence",
        codec: RV_CODEC_R_F,
        format: RV_FMT_PRED_SUCC,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fence.i",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lwu",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ld",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sd",
        codec: RV_CODEC_S,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "addiw",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &RVCP_ADDIW,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "slliw",
        codec: RV_CODEC_I_SH5,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srliw",
        codec: RV_CODEC_I_SH5,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sraiw",
        codec: RV_CODEC_I_SH5,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "addw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "subw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &RVCP_SUBW,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sllw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srlw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sraw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ldu",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lq",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sq",
        codec: RV_CODEC_S,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "addid",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sllid",
        codec: RV_CODEC_I_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srlid",
        codec: RV_CODEC_I_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sraid",
        codec: RV_CODEC_I_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "addd",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "subd",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "slld",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srld",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "srad",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mul",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mulh",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mulhsu",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mulhu",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "div",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "divu",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rem",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "remu",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mulw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "divw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "divuw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "remw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "remuw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "muld",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "divd",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "divud",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "remd",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "remud",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lr.w",
        codec: RV_CODEC_R_L,
        format: RV_FMT_AQRL_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sc.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoswap.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoadd.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoxor.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoor.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoand.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomin.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomax.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amominu.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomaxu.w",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lr.d",
        codec: RV_CODEC_R_L,
        format: RV_FMT_AQRL_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sc.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoswap.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoadd.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoxor.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoor.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoand.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomin.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomax.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amominu.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomaxu.d",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "lr.q",
        codec: RV_CODEC_R_L,
        format: RV_FMT_AQRL_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sc.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoswap.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoadd.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoxor.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoor.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amoand.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomin.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomax.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amominu.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "amomaxu.q",
        codec: RV_CODEC_R_A,
        format: RV_FMT_AQRL_RD_RS2_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ecall",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ebreak",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "uret",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sret",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "hret",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mret",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "dret",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sfence.vm",
        codec: RV_CODEC_R,
        format: RV_FMT_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sfence.vma",
        codec: RV_CODEC_R,
        format: RV_FMT_RS1_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "wfi",
        codec: RV_CODEC_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrw",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_RS1,
        pseudo: &RVCP_CSRRW,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrs",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_RS1,
        pseudo: &RVCP_CSRRS,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrc",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrwi",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_ZIMM,
        pseudo: &RVCP_CSRRWI,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrsi",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_ZIMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "csrrci",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_CSR_ZIMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "flw",
        codec: RV_CODEC_I,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsw",
        codec: RV_CODEC_S,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmadd.s",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmsub.s",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmsub.s",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmadd.s",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fadd.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsub.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmul.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fdiv.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnj.s",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJ_S,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjn.s",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJN_S,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjx.s",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJX_S,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmin.s",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmax.s",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsqrt.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fle.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "flt.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "feq.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.w.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.wu.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.w",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.wu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.x.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fclass.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.s.x",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.l.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.lu.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.l",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.lu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fld",
        codec: RV_CODEC_I,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsd",
        codec: RV_CODEC_S,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmadd.d",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmsub.d",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmsub.d",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmadd.d",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fadd.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsub.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmul.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fdiv.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnj.d",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJ_D,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjn.d",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJN_D,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjx.d",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJX_D,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmin.d",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmax.d",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsqrt.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fle.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "flt.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "feq.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.w.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.wu.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.w",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.wu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fclass.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.l.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.lu.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.x.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.l",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.lu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.d.x",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "flq",
        codec: RV_CODEC_I,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsq",
        codec: RV_CODEC_S,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmadd.q",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmsub.q",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmsub.q",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fnmadd.q",
        codec: RV_CODEC_R4_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2_FRS3,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fadd.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsub.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmul.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fdiv.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnj.q",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJ_Q,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjn.q",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJN_Q,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsgnjx.q",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &RVCP_FSGNJX_Q,
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmin.q",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmax.q",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.s.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.s",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.d.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.d",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsqrt.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fle.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "flt.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "feq.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1_FRS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.w.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.wu.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.w",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.wu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fclass.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.l.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.lu.q",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.l",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fcvt.q.lu",
        codec: RV_CODEC_R_M,
        format: RV_FMT_RM_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.x.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_FRS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.q.x",
        codec: RV_CODEC_R,
        format: RV_FMT_FRD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.addi4spn",
        codec: RV_CODEC_CIW_4SPN,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.fld",
        codec: RV_CODEC_CL_LD,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fld),
        decomp_rv64: Some(RvOp::Fld),
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.lw",
        codec: RV_CODEC_CL_LW,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Lw),
        decomp_rv64: Some(RvOp::Lw),
        decomp_rv128: Some(RvOp::Lw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.flw",
        codec: RV_CODEC_CL_LW,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Flw),
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.fsd",
        codec: RV_CODEC_CS_SD,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fsd),
        decomp_rv64: Some(RvOp::Fsd),
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.sw",
        codec: RV_CODEC_CS_SW,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Sw),
        decomp_rv64: Some(RvOp::Sw),
        decomp_rv128: Some(RvOp::Sw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.fsw",
        codec: RV_CODEC_CS_SW,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fsw),
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.nop",
        codec: RV_CODEC_CI_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.addi",
        codec: RV_CODEC_CI,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::NzHint,
    },
    RvOpcodeData {
        name: "c.jal",
        codec: RV_CODEC_CJ_JAL,
        format: RD_FMT_RD_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Jal),
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.li",
        codec: RV_CODEC_CI_LI,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.addi16sp",
        codec: RV_CODEC_CI_16SP,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.lui",
        codec: RV_CODEC_CI_LUI,
        format: RV_FMT_RD_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Lui),
        decomp_rv64: Some(RvOp::Lui),
        decomp_rv128: Some(RvOp::Lui),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.srli",
        codec: RV_CODEC_CB_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Srli),
        decomp_rv64: Some(RvOp::Srli),
        decomp_rv128: Some(RvOp::Srli),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.srai",
        codec: RV_CODEC_CB_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Srai),
        decomp_rv64: Some(RvOp::Srai),
        decomp_rv128: Some(RvOp::Srai),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.andi",
        codec: RV_CODEC_CB_IMM,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Andi),
        decomp_rv64: Some(RvOp::Andi),
        decomp_rv128: Some(RvOp::Andi),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.sub",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Sub),
        decomp_rv64: Some(RvOp::Sub),
        decomp_rv128: Some(RvOp::Sub),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.xor",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Xor),
        decomp_rv64: Some(RvOp::Xor),
        decomp_rv128: Some(RvOp::Xor),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.or",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Or),
        decomp_rv64: Some(RvOp::Or),
        decomp_rv128: Some(RvOp::Or),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.and",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::And),
        decomp_rv64: Some(RvOp::And),
        decomp_rv128: Some(RvOp::And),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.subw",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Subw),
        decomp_rv64: Some(RvOp::Subw),
        decomp_rv128: Some(RvOp::Subw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.addw",
        codec: RV_CODEC_CS,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addw),
        decomp_rv64: Some(RvOp::Addw),
        decomp_rv128: Some(RvOp::Addw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.j",
        codec: RV_CODEC_CJ,
        format: RD_FMT_RD_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Jal),
        decomp_rv64: Some(RvOp::Jal),
        decomp_rv128: Some(RvOp::Jal),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.beqz",
        codec: RV_CODEC_CB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Beq),
        decomp_rv64: Some(RvOp::Beq),
        decomp_rv128: Some(RvOp::Beq),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.bnez",
        codec: RV_CODEC_CB,
        format: RV_FMT_RS1_RS2_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Bne),
        decomp_rv64: Some(RvOp::Bne),
        decomp_rv128: Some(RvOp::Bne),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.slli",
        codec: RV_CODEC_CI_SH6,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Slli),
        decomp_rv64: Some(RvOp::Slli),
        decomp_rv128: Some(RvOp::Slli),
        decomp_data: RvCdImmediate::Nz,
    },
    RvOpcodeData {
        name: "c.fldsp",
        codec: RV_CODEC_CI_LDSP,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fld),
        decomp_rv64: Some(RvOp::Fld),
        decomp_rv128: Some(RvOp::Fld),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.lwsp",
        codec: RV_CODEC_CI_LWSP,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Lw),
        decomp_rv64: Some(RvOp::Lw),
        decomp_rv128: Some(RvOp::Lw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.flwsp",
        codec: RV_CODEC_CI_LWSP,
        format: RV_FMT_FRD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Flw),
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.jr",
        codec: RV_CODEC_CR_JR,
        format: RV_FMT_RD_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Jalr),
        decomp_rv64: Some(RvOp::Jalr),
        decomp_rv128: Some(RvOp::Jalr),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.mv",
        codec: RV_CODEC_CR_MV,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Addi),
        decomp_rv64: Some(RvOp::Addi),
        decomp_rv128: Some(RvOp::Addi),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.ebreak",
        codec: RV_CODEC_CI_NONE,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Ebreak),
        decomp_rv64: Some(RvOp::Ebreak),
        decomp_rv128: Some(RvOp::Ebreak),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.jalr",
        codec: RV_CODEC_CR_JALR,
        format: RV_FMT_RD_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Jalr),
        decomp_rv64: Some(RvOp::Jalr),
        decomp_rv128: Some(RvOp::Jalr),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.add",
        codec: RV_CODEC_CR,
        format: RV_FMT_RD_RS1_RS2,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Add),
        decomp_rv64: Some(RvOp::Add),
        decomp_rv128: Some(RvOp::Add),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.fsdsp",
        codec: RV_CODEC_CSS_SDSP,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fsd),
        decomp_rv64: Some(RvOp::Fsd),
        decomp_rv128: Some(RvOp::Fsd),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.swsp",
        codec: RV_CODEC_CSS_SWSP,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Sw),
        decomp_rv64: Some(RvOp::Sw),
        decomp_rv128: Some(RvOp::Sw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.fswsp",
        codec: RV_CODEC_CSS_SWSP,
        format: RV_FMT_FRS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: Some(RvOp::Fsw),
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.ld",
        codec: RV_CODEC_CL_LD,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: Some(RvOp::Ld),
        decomp_rv128: Some(RvOp::Ld),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.sd",
        codec: RV_CODEC_CS_SD,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: Some(RvOp::Sd),
        decomp_rv128: Some(RvOp::Sd),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.addiw",
        codec: RV_CODEC_CI,
        format: RV_FMT_RD_RS1_IMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: Some(RvOp::Addiw),
        decomp_rv128: Some(RvOp::Addiw),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.ldsp",
        codec: RV_CODEC_CI_LDSP,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: Some(RvOp::Ld),
        decomp_rv128: Some(RvOp::Ld),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.sdsp",
        codec: RV_CODEC_CSS_SDSP,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: Some(RvOp::Sd),
        decomp_rv128: Some(RvOp::Sd),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.lq",
        codec: RV_CODEC_CL_LQ,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: Some(RvOp::Lq),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.sq",
        codec: RV_CODEC_CS_SQ,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: Some(RvOp::Sq),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.lqsp",
        codec: RV_CODEC_CI_LQSP,
        format: RV_FMT_RD_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: Some(RvOp::Lq),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "c.sqsp",
        codec: RV_CODEC_RSS_SQSP,
        format: RV_FMT_RS2_OFFSET_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: Some(RvOp::Sq),
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "nop",
        codec: RV_CODEC_I,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "mv",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "not",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "neg",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "negw",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sext.w",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "seqz",
        codec: RV_CODEC_I,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "snez",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sltz",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "sgtz",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS2,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fabs.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fneg.s",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fabs.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fneg.d",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fmv.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fabs.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fneg.q",
        codec: RV_CODEC_R,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "beqz",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bnez",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "blez",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bgez",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bltz",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bgtz",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ble",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bleu",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bgt",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "bgtu",
        codec: RV_CODEC_SB,
        format: RV_FMT_RS2_RS1_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "j",
        codec: RV_CODEC_UJ,
        format: RV_FMT_OFFSET,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "ret",
        codec: RV_CODEC_I,
        format: RV_FMT_NONE,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "jr",
        codec: RV_CODEC_I,
        format: RV_FMT_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdcycle",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdtime",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdinstret",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdcycleh",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdtimeh",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "rdinstreth",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "frcsr",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "frrm",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "frflags",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fscsr",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsrm",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsflags",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_RS1,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsrmi",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_ZIMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
    RvOpcodeData {
        name: "fsflagsi",
        codec: RV_CODEC_I_CSR,
        format: RV_FMT_RD_ZIMM,
        pseudo: &[],
        decomp_rv32: None,
        decomp_rv64: None,
        decomp_rv128: None,
        decomp_data: RvCdImmediate::None,
    },
];

fn csr_name(csrno: i32) -> Option<&'static str> {
    match csrno {
        0 => Some("ustatus"),
        1 => Some("fflags"),
        2 => Some("frm"),
        3 => Some("fcsr"),
        4 => Some("uie"),
        5 => Some("utvec"),
        7 => Some("utvt"),
        8 => Some("vstart"),
        9 => Some("vxsat"),
        10 => Some("vxrm"),
        15 => Some("vcsr"),
        64 => Some("uscratch"),
        65 => Some("uepc"),
        66 => Some("ucause"),
        67 => Some("utval"),
        68 => Some("uip"),
        69 => Some("unxti"),
        70 => Some("uintstatus"),
        72 => Some("uscratchcsw"),
        73 => Some("uscratchcswl"),
        256 => Some("sstatus"),
        258 => Some("sedeleg"),
        259 => Some("sideleg"),
        260 => Some("sie"),
        261 => Some("stvec"),
        262 => Some("scounteren"),
        263 => Some("stvt"),
        320 => Some("sscratch"),
        321 => Some("sepc"),
        322 => Some("scause"),
        323 => Some("stval"),
        324 => Some("sip"),
        325 => Some("snxti"),
        326 => Some("sintstatus"),
        328 => Some("sscratchcsw"),
        329 => Some("sscratchcswl"),
        384 => Some("satp"),
        512 => Some("vsstatus"),
        516 => Some("vsie"),
        517 => Some("vstvec"),
        576 => Some("vsscratch"),
        577 => Some("vsepc"),
        578 => Some("vscause"),
        579 => Some("vstval"),
        580 => Some("vsip"),
        640 => Some("vsatp"),
        768 => Some("mstatus"),
        769 => Some("misa"),
        770 => Some("medeleg"),
        771 => Some("mideleg"),
        772 => Some("mie"),
        773 => Some("mtvec"),
        774 => Some("mcounteren"),
        775 => Some("mtvt"),
        784 => Some("mstatush"),
        800 => Some("mcountinhibit"),
        803 => Some("mhpmevent3"),
        804 => Some("mhpmevent4"),
        805 => Some("mhpmevent5"),
        806 => Some("mhpmevent6"),
        807 => Some("mhpmevent7"),
        808 => Some("mhpmevent8"),
        809 => Some("mhpmevent9"),
        810 => Some("mhpmevent10"),
        811 => Some("mhpmevent11"),
        812 => Some("mhpmevent12"),
        813 => Some("mhpmevent13"),
        814 => Some("mhpmevent14"),
        815 => Some("mhpmevent15"),
        816 => Some("mhpmevent16"),
        817 => Some("mhpmevent17"),
        818 => Some("mhpmevent18"),
        819 => Some("mhpmevent19"),
        820 => Some("mhpmevent20"),
        821 => Some("mhpmevent21"),
        822 => Some("mhpmevent22"),
        823 => Some("mhpmevent23"),
        824 => Some("mhpmevent24"),
        825 => Some("mhpmevent25"),
        826 => Some("mhpmevent26"),
        827 => Some("mhpmevent27"),
        828 => Some("mhpmevent28"),
        829 => Some("mhpmevent29"),
        830 => Some("mhpmevent30"),
        831 => Some("mhpmevent31"),
        832 => Some("mscratch"),
        833 => Some("mepc"),
        834 => Some("mcause"),
        835 => Some("mtval"),
        836 => Some("mip"),
        837 => Some("mnxti"),
        838 => Some("mintstatus"),
        840 => Some("mscratchcsw"),
        841 => Some("mscratchcswl"),
        842 => Some("mtinst"),
        843 => Some("mtval2"),
        928 => Some("pmpcfg0"),
        929 => Some("pmpcfg1"),
        930 => Some("pmpcfg2"),
        931 => Some("pmpcfg3"),
        944 => Some("pmpaddr0"),
        945 => Some("pmpaddr1"),
        946 => Some("pmpaddr2"),
        947 => Some("pmpaddr3"),
        948 => Some("pmpaddr4"),
        949 => Some("pmpaddr5"),
        950 => Some("pmpaddr6"),
        951 => Some("pmpaddr7"),
        952 => Some("pmpaddr8"),
        953 => Some("pmpaddr9"),
        954 => Some("pmpaddr10"),
        955 => Some("pmpaddr11"),
        956 => Some("pmpaddr12"),
        957 => Some("pmpaddr13"),
        958 => Some("pmpaddr14"),
        959 => Some("pmpaddr15"),
        1536 => Some("hstatus"),
        1538 => Some("hedeleg"),
        1539 => Some("hideleg"),
        1540 => Some("hie"),
        1541 => Some("htimedelta"),
        1542 => Some("hcounteren"),
        1543 => Some("hgeie"),
        1557 => Some("htimedeltah"),
        1603 => Some("htval"),
        1604 => Some("hip"),
        1605 => Some("hvip"),
        1610 => Some("htinst"),
        1664 => Some("hgatp"),
        1952 => Some("tselect"),
        1953 => Some("tdata1"),
        1954 => Some("tdata2"),
        1955 => Some("tdata3"),
        1956 => Some("tinfo"),
        1957 => Some("tcontrol"),
        1960 => Some("mcontext"),
        1961 => Some("mnoise"),
        1962 => Some("scontext"),
        1968 => Some("dcsr"),
        1969 => Some("dpc"),
        1970 => Some("dscratch0"),
        1971 => Some("dscratch1"),
        2816 => Some("mcycle"),
        2818 => Some("minstret"),
        2819 => Some("mhpmcounter3"),
        2820 => Some("mhpmcounter4"),
        2821 => Some("mhpmcounter5"),
        2822 => Some("mhpmcounter6"),
        2823 => Some("mhpmcounter7"),
        2824 => Some("mhpmcounter8"),
        2825 => Some("mhpmcounter9"),
        2826 => Some("mhpmcounter10"),
        2827 => Some("mhpmcounter11"),
        2828 => Some("mhpmcounter12"),
        2829 => Some("mhpmcounter13"),
        2830 => Some("mhpmcounter14"),
        2831 => Some("mhpmcounter15"),
        2832 => Some("mhpmcounter16"),
        2833 => Some("mhpmcounter17"),
        2834 => Some("mhpmcounter18"),
        2835 => Some("mhpmcounter19"),
        2836 => Some("mhpmcounter20"),
        2837 => Some("mhpmcounter21"),
        2838 => Some("mhpmcounter22"),
        2839 => Some("mhpmcounter23"),
        2840 => Some("mhpmcounter24"),
        2841 => Some("mhpmcounter25"),
        2842 => Some("mhpmcounter26"),
        2843 => Some("mhpmcounter27"),
        2844 => Some("mhpmcounter28"),
        2845 => Some("mhpmcounter29"),
        2846 => Some("mhpmcounter30"),
        2847 => Some("mhpmcounter31"),
        2944 => Some("mcycleh"),
        2946 => Some("minstreth"),
        2947 => Some("mhpmcounter3h"),
        2948 => Some("mhpmcounter4h"),
        2949 => Some("mhpmcounter5h"),
        2950 => Some("mhpmcounter6h"),
        2951 => Some("mhpmcounter7h"),
        2952 => Some("mhpmcounter8h"),
        2953 => Some("mhpmcounter9h"),
        2954 => Some("mhpmcounter10h"),
        2955 => Some("mhpmcounter11h"),
        2956 => Some("mhpmcounter12h"),
        2957 => Some("mhpmcounter13h"),
        2958 => Some("mhpmcounter14h"),
        2959 => Some("mhpmcounter15h"),
        2960 => Some("mhpmcounter16h"),
        2961 => Some("mhpmcounter17h"),
        2962 => Some("mhpmcounter18h"),
        2963 => Some("mhpmcounter19h"),
        2964 => Some("mhpmcounter20h"),
        2965 => Some("mhpmcounter21h"),
        2966 => Some("mhpmcounter22h"),
        2967 => Some("mhpmcounter23h"),
        2968 => Some("mhpmcounter24h"),
        2969 => Some("mhpmcounter25h"),
        2970 => Some("mhpmcounter26h"),
        2971 => Some("mhpmcounter27h"),
        2972 => Some("mhpmcounter28h"),
        2973 => Some("mhpmcounter29h"),
        2974 => Some("mhpmcounter30h"),
        2975 => Some("mhpmcounter31h"),
        3072 => Some("cycle"),
        3073 => Some("time"),
        3074 => Some("instret"),
        3075 => Some("hpmcounter3"),
        3076 => Some("hpmcounter4"),
        3077 => Some("hpmcounter5"),
        3078 => Some("hpmcounter6"),
        3079 => Some("hpmcounter7"),
        3080 => Some("hpmcounter8"),
        3081 => Some("hpmcounter9"),
        3082 => Some("hpmcounter10"),
        3083 => Some("hpmcounter11"),
        3084 => Some("hpmcounter12"),
        3085 => Some("hpmcounter13"),
        3086 => Some("hpmcounter14"),
        3087 => Some("hpmcounter15"),
        3088 => Some("hpmcounter16"),
        3089 => Some("hpmcounter17"),
        3090 => Some("hpmcounter18"),
        3091 => Some("hpmcounter19"),
        3092 => Some("hpmcounter20"),
        3093 => Some("hpmcounter21"),
        3094 => Some("hpmcounter22"),
        3095 => Some("hpmcounter23"),
        3096 => Some("hpmcounter24"),
        3097 => Some("hpmcounter25"),
        3098 => Some("hpmcounter26"),
        3099 => Some("hpmcounter27"),
        3100 => Some("hpmcounter28"),
        3101 => Some("hpmcounter29"),
        3102 => Some("hpmcounter30"),
        3103 => Some("hpmcounter31"),
        3104 => Some("vl"),
        3105 => Some("vtype"),
        3106 => Some("vlenb"),
        3200 => Some("cycleh"),
        3201 => Some("timeh"),
        3202 => Some("instreth"),
        3203 => Some("hpmcounter3h"),
        3204 => Some("hpmcounter4h"),
        3205 => Some("hpmcounter5h"),
        3206 => Some("hpmcounter6h"),
        3207 => Some("hpmcounter7h"),
        3208 => Some("hpmcounter8h"),
        3209 => Some("hpmcounter9h"),
        3210 => Some("hpmcounter10h"),
        3211 => Some("hpmcounter11h"),
        3212 => Some("hpmcounter12h"),
        3213 => Some("hpmcounter13h"),
        3214 => Some("hpmcounter14h"),
        3215 => Some("hpmcounter15h"),
        3216 => Some("hpmcounter16h"),
        3217 => Some("hpmcounter17h"),
        3218 => Some("hpmcounter18h"),
        3219 => Some("hpmcounter19h"),
        3220 => Some("hpmcounter20h"),
        3221 => Some("hpmcounter21h"),
        3222 => Some("hpmcounter22h"),
        3223 => Some("hpmcounter23h"),
        3224 => Some("hpmcounter24h"),
        3225 => Some("hpmcounter25h"),
        3226 => Some("hpmcounter26h"),
        3227 => Some("hpmcounter27h"),
        3228 => Some("hpmcounter28h"),
        3229 => Some("hpmcounter29h"),
        3230 => Some("hpmcounter30h"),
        3231 => Some("hpmcounter31h"),
        3602 => Some("hgeip"),
        3857 => Some("mvendorid"),
        3858 => Some("marchid"),
        3859 => Some("mimpid"),
        3860 => Some("mhartid"),
        3861 => Some("mentropy"),
        _ => None,
    }
}

fn decode_inst_opcode(dec: &mut RvDecode, isa: RvIsa) {
    let inst = dec.inst;
    let mut op = RvOp::Illegal;
    match inst >> 0 & 0o3 {
        0 => match inst >> 13 & 0o7 {
            0 => {
                op = RvOp::CAddi4spn;
            }
            1 => {
                op = if isa == RvIsa::Rv128 {
                    RvOp::CLq
                } else {
                    RvOp::CFld
                };
            }
            2 => {
                op = RvOp::CLw;
            }
            3 => {
                op = if isa == RvIsa::Rv32 {
                    RvOp::CFlw
                } else {
                    RvOp::CLd
                };
            }
            5 => {
                op = if isa == RvIsa::Rv128 {
                    RvOp::CSq
                } else {
                    RvOp::CFsd
                };
            }
            6 => {
                op = RvOp::CSw;
            }
            7 => {
                op = if isa == RvIsa::Rv32 {
                    RvOp::CFsw
                } else {
                    RvOp::CSd
                };
            }
            _ => {}
        },
        1 => match inst >> 13 & 0o7 {
            0 => match inst >> 2 & 0o3777 {
                0 => {
                    op = RvOp::CNop;
                }
                _ => {
                    op = RvOp::CAddi;
                }
            },
            1 => {
                op = if isa == RvIsa::Rv32 {
                    RvOp::CJal
                } else {
                    RvOp::CAddiw
                };
            }
            2 => {
                op = RvOp::CLi;
            }
            3 => match inst >> 7 & 0o37 {
                2 => {
                    op = RvOp::CAddi16sp;
                }
                _ => {
                    op = RvOp::CLui;
                }
            },
            4 => match inst >> 10 & 0o3 {
                0 => {
                    op = RvOp::CSrli;
                }
                1 => {
                    op = RvOp::CSrai;
                }
                2 => {
                    op = RvOp::CAndi;
                }
                3 => match inst >> 10 & 0o4 as u64 | inst >> 5 & 0o3 as u64 {
                    0 => {
                        op = RvOp::CSub;
                    }
                    1 => {
                        op = RvOp::CXor;
                    }
                    2 => {
                        op = RvOp::COr;
                    }
                    3 => {
                        op = RvOp::CAnd;
                    }
                    4 => {
                        op = RvOp::CSubw;
                    }
                    5 => {
                        op = RvOp::CAddw;
                    }
                    _ => {}
                },
                _ => {}
            },
            5 => {
                op = RvOp::CJ;
            }
            6 => {
                op = RvOp::CBeqz;
            }
            7 => {
                op = RvOp::CBnez;
            }
            _ => {}
        },
        2 => match inst >> 13 & 0o7 as u64 {
            0 => {
                op = RvOp::CSlli;
            }
            1 => {
                op = if isa == RvIsa::Rv128 {
                    RvOp::CLqsp
                } else {
                    RvOp::CFldsp
                };
            }
            2 => {
                op = RvOp::CLwsp;
            }
            3 => {
                op = if isa == RvIsa::Rv32 {
                    RvOp::CFlwsp
                } else {
                    RvOp::CLdsp
                };
            }
            4 => match inst >> 12 & 0o1 as u64 {
                0 => match inst >> 2 & 0o37 as u64 {
                    0 => {
                        op = RvOp::CJr;
                    }
                    _ => {
                        op = RvOp::CMv;
                    }
                },
                1 => match inst >> 2 & 0o37 as u64 {
                    0 => match inst >> 7 & 0o37 as u64 {
                        0 => {
                            op = RvOp::CEbreak;
                        }
                        _ => {
                            op = RvOp::CJalr;
                        }
                    },
                    _ => {
                        op = RvOp::CAdd;
                    }
                },
                _ => {}
            },
            5 => {
                op = if isa == RvIsa::Rv128 {
                    RvOp::CSqsp
                } else {
                    RvOp::CFsdsp
                };
            }
            6 => {
                op = RvOp::CSwsp;
            }
            7 => {
                op = if isa == RvIsa::Rv32 {
                    RvOp::CFswsp
                } else {
                    RvOp::CSdsp
                };
            }
            _ => {}
        },
        3 => match inst >> 2 & 0o37 as u64 {
            0 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Lb;
                }
                1 => {
                    op = RvOp::Lh;
                }
                2 => {
                    op = RvOp::Lw;
                }
                3 => {
                    op = RvOp::Ld;
                }
                4 => {
                    op = RvOp::Lbu;
                }
                5 => {
                    op = RvOp::Lhu;
                }
                6 => {
                    op = RvOp::Lwu;
                }
                7 => {
                    op = RvOp::Ldu;
                }
                _ => {}
            },
            1 => match inst >> 12 & 0o7 as u64 {
                2 => {
                    op = RvOp::Flw;
                }
                3 => {
                    op = RvOp::Fld;
                }
                4 => {
                    op = RvOp::Flq;
                }
                _ => {}
            },
            3 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Fence;
                }
                1 => {
                    op = RvOp::FenceI;
                }
                2 => {
                    op = RvOp::Lq;
                }
                _ => {}
            },
            4 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Addi;
                }
                1 => match inst >> 27 & 0o37 as u64 {
                    0 => {
                        op = RvOp::Slli;
                    }
                    _ => {}
                },
                2 => {
                    op = RvOp::Slti;
                }
                3 => {
                    op = RvOp::Sltiu;
                }
                4 => {
                    op = RvOp::Xori;
                }
                5 => match inst >> 27 & 0o37 as u64 {
                    0 => {
                        op = RvOp::Srli;
                    }
                    8 => {
                        op = RvOp::Srai;
                    }
                    _ => {}
                },
                6 => {
                    op = RvOp::Ori;
                }
                7 => {
                    op = RvOp::Andi;
                }
                _ => {}
            },
            5 => {
                op = RvOp::Auipc;
            }
            6 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Addiw;
                }
                1 => match inst >> 25 & 0o177 as u64 {
                    0 => {
                        op = RvOp::Slliw;
                    }
                    _ => {}
                },
                5 => match inst >> 25 & 0o177 as u64 {
                    0 => {
                        op = RvOp::Srliw;
                    }
                    32 => {
                        op = RvOp::Sraiw;
                    }
                    _ => {}
                },
                _ => {}
            },
            8 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Sb;
                }
                1 => {
                    op = RvOp::Sh;
                }
                2 => {
                    op = RvOp::Sw;
                }
                3 => {
                    op = RvOp::Sd;
                }
                4 => {
                    op = RvOp::Sq;
                }
                _ => {}
            },
            9 => match inst >> 12 & 0o7 as u64 {
                2 => {
                    op = RvOp::Fsw;
                }
                3 => {
                    op = RvOp::Fsd;
                }
                4 => {
                    op = RvOp::Fsq;
                }
                _ => {}
            },
            11 => match inst >> 24 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                2 => {
                    op = RvOp::AmoaddW;
                }
                3 => {
                    op = RvOp::AmoaddD;
                }
                4 => {
                    op = RvOp::AmoaddQ;
                }
                10 => {
                    op = RvOp::AmoswapW;
                }
                11 => {
                    op = RvOp::AmoswapD;
                }
                12 => {
                    op = RvOp::AmoswapQ;
                }
                18 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::LrW;
                    }
                    _ => {}
                },
                19 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::LrD;
                    }
                    _ => {}
                },
                20 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::LrQ;
                    }
                    _ => {}
                },
                26 => {
                    op = RvOp::ScW;
                }
                27 => {
                    op = RvOp::ScD;
                }
                28 => {
                    op = RvOp::ScQ;
                }
                34 => {
                    op = RvOp::AmoxorW;
                }
                35 => {
                    op = RvOp::AmoxorD;
                }
                36 => {
                    op = RvOp::AmoxorQ;
                }
                66 => {
                    op = RvOp::AmoorW;
                }
                67 => {
                    op = RvOp::AmoorD;
                }
                68 => {
                    op = RvOp::AmoorQ;
                }
                98 => {
                    op = RvOp::AmoandW;
                }
                99 => {
                    op = RvOp::AmoandD;
                }
                100 => {
                    op = RvOp::AmoandQ;
                }
                130 => {
                    op = RvOp::AmominW;
                }
                131 => {
                    op = RvOp::AmominD;
                }
                132 => {
                    op = RvOp::AmominQ;
                }
                162 => {
                    op = RvOp::AmomaxW;
                }
                163 => {
                    op = RvOp::AmomaxD;
                }
                164 => {
                    op = RvOp::AmomaxQ;
                }
                194 => {
                    op = RvOp::AmominuW;
                }
                195 => {
                    op = RvOp::AmominuD;
                }
                196 => {
                    op = RvOp::AmominuQ;
                }
                226 => {
                    op = RvOp::AmomaxuW;
                }
                227 => {
                    op = RvOp::AmomaxuD;
                }
                228 => {
                    op = RvOp::AmomaxuQ;
                }
                _ => {}
            },
            12 => match inst >> 22 & 0o1770 as u64 | inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Add;
                }
                1 => {
                    op = RvOp::Sll;
                }
                2 => {
                    op = RvOp::Slt;
                }
                3 => {
                    op = RvOp::Sltu;
                }
                4 => {
                    op = RvOp::Xor;
                }
                5 => {
                    op = RvOp::Srl;
                }
                6 => {
                    op = RvOp::Or;
                }
                7 => {
                    op = RvOp::And;
                }
                8 => {
                    op = RvOp::Mul;
                }
                9 => {
                    op = RvOp::Mulh;
                }
                10 => {
                    op = RvOp::Mulhsu;
                }
                11 => {
                    op = RvOp::Mulhu;
                }
                12 => {
                    op = RvOp::Div;
                }
                13 => {
                    op = RvOp::Divu;
                }
                14 => {
                    op = RvOp::Rem;
                }
                15 => {
                    op = RvOp::Remu;
                }
                256 => {
                    op = RvOp::Sub;
                }
                261 => {
                    op = RvOp::Sra;
                }
                _ => {}
            },
            13 => {
                op = RvOp::Lui;
            }
            14 => match inst >> 22 & 0o1770 as u64 | inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Addw;
                }
                1 => {
                    op = RvOp::Sllw;
                }
                5 => {
                    op = RvOp::Srlw;
                }
                8 => {
                    op = RvOp::Mulw;
                }
                12 => {
                    op = RvOp::Divw;
                }
                13 => {
                    op = RvOp::Divuw;
                }
                14 => {
                    op = RvOp::Remw;
                }
                15 => {
                    op = RvOp::Remuw;
                }
                256 => {
                    op = RvOp::Subw;
                }
                261 => {
                    op = RvOp::Sraw;
                }
                _ => {}
            },
            16 => match inst >> 25 & 0o3 as u64 {
                0 => {
                    op = RvOp::FmaddS;
                }
                1 => {
                    op = RvOp::FmaddD;
                }
                3 => {
                    op = RvOp::FmaddQ;
                }
                _ => {}
            },
            17 => match inst >> 25 & 0o3 as u64 {
                0 => {
                    op = RvOp::FmsubS;
                }
                1 => {
                    op = RvOp::FmsubD;
                }
                3 => {
                    op = RvOp::FmsubQ;
                }
                _ => {}
            },
            18 => match inst >> 25 & 0o3 as u64 {
                0 => {
                    op = RvOp::FnmsubS;
                }
                1 => {
                    op = RvOp::FnmsubD;
                }
                3 => {
                    op = RvOp::FnmsubQ;
                }
                _ => {}
            },
            19 => match inst >> 25 & 0o3 as u64 {
                0 => {
                    op = RvOp::FnmaddS;
                }
                1 => {
                    op = RvOp::FnmaddD;
                }
                3 => {
                    op = RvOp::FnmaddQ;
                }
                _ => {}
            },
            20 => match inst >> 25 & 0o177 as u64 {
                0 => {
                    op = RvOp::FaddS;
                }
                1 => {
                    op = RvOp::FaddD;
                }
                3 => {
                    op = RvOp::FaddQ;
                }
                4 => {
                    op = RvOp::FsubS;
                }
                5 => {
                    op = RvOp::FsubD;
                }
                7 => {
                    op = RvOp::FsubQ;
                }
                8 => {
                    op = RvOp::FmulS;
                }
                9 => {
                    op = RvOp::FmulD;
                }
                11 => {
                    op = RvOp::FmulQ;
                }
                12 => {
                    op = RvOp::FdivS;
                }
                13 => {
                    op = RvOp::FdivD;
                }
                15 => {
                    op = RvOp::FdivQ;
                }
                16 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FsgnjS;
                    }
                    1 => {
                        op = RvOp::FsgnjnS;
                    }
                    2 => {
                        op = RvOp::FsgnjxS;
                    }
                    _ => {}
                },
                17 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FsgnjD;
                    }
                    1 => {
                        op = RvOp::FsgnjnD;
                    }
                    2 => {
                        op = RvOp::FsgnjxD;
                    }
                    _ => {}
                },
                19 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FsgnjQ;
                    }
                    1 => {
                        op = RvOp::FsgnjnQ;
                    }
                    2 => {
                        op = RvOp::FsgnjxQ;
                    }
                    _ => {}
                },
                20 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FminS;
                    }
                    1 => {
                        op = RvOp::FmaxS;
                    }
                    _ => {}
                },
                21 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FminD;
                    }
                    1 => {
                        op = RvOp::FmaxD;
                    }
                    _ => {}
                },
                23 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FminQ;
                    }
                    1 => {
                        op = RvOp::FmaxQ;
                    }
                    _ => {}
                },
                32 => match inst >> 20 & 0o37 as u64 {
                    1 => {
                        op = RvOp::FcvtSD;
                    }
                    3 => {
                        op = RvOp::FcvtSQ;
                    }
                    _ => {}
                },
                33 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtDS;
                    }
                    3 => {
                        op = RvOp::FcvtDQ;
                    }
                    _ => {}
                },
                35 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtQS;
                    }
                    1 => {
                        op = RvOp::FcvtQD;
                    }
                    _ => {}
                },
                44 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FsqrtS;
                    }
                    _ => {}
                },
                45 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FsqrtD;
                    }
                    _ => {}
                },
                47 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FsqrtQ;
                    }
                    _ => {}
                },
                80 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FleS;
                    }
                    1 => {
                        op = RvOp::FltS;
                    }
                    2 => {
                        op = RvOp::FeqS;
                    }
                    _ => {}
                },
                81 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FleD;
                    }
                    1 => {
                        op = RvOp::FltD;
                    }
                    2 => {
                        op = RvOp::FeqD;
                    }
                    _ => {}
                },
                83 => match inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FleQ;
                    }
                    1 => {
                        op = RvOp::FltQ;
                    }
                    2 => {
                        op = RvOp::FeqQ;
                    }
                    _ => {}
                },
                96 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtWS;
                    }
                    1 => {
                        op = RvOp::FcvtWuS;
                    }
                    2 => {
                        op = RvOp::FcvtLS;
                    }
                    3 => {
                        op = RvOp::FcvtLuS;
                    }
                    _ => {}
                },
                97 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtWD;
                    }
                    1 => {
                        op = RvOp::FcvtWuD;
                    }
                    2 => {
                        op = RvOp::FcvtLD;
                    }
                    3 => {
                        op = RvOp::FcvtLuD;
                    }
                    _ => {}
                },
                99 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtWQ;
                    }
                    1 => {
                        op = RvOp::FcvtWuQ;
                    }
                    2 => {
                        op = RvOp::FcvtLQ;
                    }
                    3 => {
                        op = RvOp::FcvtLuQ;
                    }
                    _ => {}
                },
                104 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtSW;
                    }
                    1 => {
                        op = RvOp::FcvtSWu;
                    }
                    2 => {
                        op = RvOp::FcvtSL;
                    }
                    3 => {
                        op = RvOp::FcvtSLu;
                    }
                    _ => {}
                },
                105 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtDW;
                    }
                    1 => {
                        op = RvOp::FcvtDWu;
                    }
                    2 => {
                        op = RvOp::FcvtDL;
                    }
                    3 => {
                        op = RvOp::FcvtDLu;
                    }
                    _ => {}
                },
                107 => match inst >> 20 & 0o37 as u64 {
                    0 => {
                        op = RvOp::FcvtQW;
                    }
                    1 => {
                        op = RvOp::FcvtQWu;
                    }
                    2 => {
                        op = RvOp::FcvtQL;
                    }
                    3 => {
                        op = RvOp::FcvtQLu;
                    }
                    _ => {}
                },
                112 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvXS;
                    }
                    1 => {
                        op = RvOp::FclassS;
                    }
                    _ => {}
                },
                113 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvXD;
                    }
                    1 => {
                        op = RvOp::FclassD;
                    }
                    _ => {}
                },
                115 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvXQ;
                    }
                    1 => {
                        op = RvOp::FclassQ;
                    }
                    _ => {}
                },
                120 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvSX;
                    }
                    _ => {}
                },
                121 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvDX;
                    }
                    _ => {}
                },
                123 => match inst >> 17 & 0o370 as u64 | inst >> 12 & 0o7 as u64 {
                    0 => {
                        op = RvOp::FmvQX;
                    }
                    _ => {}
                },
                _ => {}
            },
            22 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Addid;
                }
                1 => match inst >> 26 & 0o77 as u64 {
                    0 => {
                        op = RvOp::Sllid;
                    }
                    _ => {}
                },
                5 => match inst >> 26 & 0o77 as u64 {
                    0 => {
                        op = RvOp::Srlid;
                    }
                    16 => {
                        op = RvOp::Sraid;
                    }
                    _ => {}
                },
                _ => {}
            },
            24 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Beq;
                }
                1 => {
                    op = RvOp::Bne;
                }
                4 => {
                    op = RvOp::Blt;
                }
                5 => {
                    op = RvOp::Bge;
                }
                6 => {
                    op = RvOp::Bltu;
                }
                7 => {
                    op = RvOp::Bgeu;
                }
                _ => {}
            },
            25 => match inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Jalr;
                }
                _ => {}
            },
            27 => {
                op = RvOp::Jal;
            }
            28 => match inst >> 12 & 0o7 as u64 {
                0 => match inst >> 20 & 0o7740 as u64 | inst >> 7 & 0o37 as u64 {
                    0 => match inst >> 15 & 0o1777 as u64 {
                        0 => {
                            op = RvOp::Ecall;
                        }
                        32 => {
                            op = RvOp::Ebreak;
                        }
                        64 => {
                            op = RvOp::Uret;
                        }
                        _ => {}
                    },
                    256 => match inst >> 20 & 0o37 as u64 {
                        2 => match inst >> 15 & 0o37 as u64 {
                            0 => {
                                op = RvOp::Sret;
                            }
                            _ => {}
                        },
                        4 => {
                            op = RvOp::SfenceVm;
                        }
                        5 => match inst >> 15 & 0o37 as u64 {
                            0 => {
                                op = RvOp::Wfi;
                            }
                            _ => {}
                        },
                        _ => {}
                    },
                    288 => {
                        op = RvOp::SfenceVma;
                    }
                    512 => match inst >> 15 & 0o1777 as u64 {
                        64 => {
                            op = RvOp::Hret;
                        }
                        _ => {}
                    },
                    768 => match inst >> 15 & 0o1777 as u64 {
                        64 => {
                            op = RvOp::Mret;
                        }
                        _ => {}
                    },
                    1952 => match inst >> 15 & 0o1777 as u64 {
                        576 => {
                            op = RvOp::Dret;
                        }
                        _ => {}
                    },
                    _ => {}
                },
                1 => {
                    op = RvOp::Csrrw;
                }
                2 => {
                    op = RvOp::Csrrs;
                }
                3 => {
                    op = RvOp::Csrrc;
                }
                5 => {
                    op = RvOp::Csrrwi;
                }
                6 => {
                    op = RvOp::Csrrsi;
                }
                7 => {
                    op = RvOp::Csrrci;
                }
                _ => {}
            },
            30 => match inst >> 22 & 0o1770 as u64 | inst >> 12 & 0o7 as u64 {
                0 => {
                    op = RvOp::Addd;
                }
                1 => {
                    op = RvOp::Slld;
                }
                5 => {
                    op = RvOp::Srld;
                }
                8 => {
                    op = RvOp::Muld;
                }
                12 => {
                    op = RvOp::Divd;
                }
                13 => {
                    op = RvOp::Divud;
                }
                14 => {
                    op = RvOp::Remd;
                }
                15 => {
                    op = RvOp::Remud;
                }
                256 => {
                    op = RvOp::Subd;
                }
                261 => {
                    op = RvOp::Srad;
                }
                _ => {}
            },
            _ => {}
        },
        _ => {}
    }
    dec.op = op;
}

fn operand_rd(inst: RvInst) -> u8 {
    (inst << 52 >> 59) as u8
}
fn operand_rs1(inst: RvInst) -> u8 {
    (inst << 44 >> 59) as u8
}
fn operand_rs2(inst: RvInst) -> u8 {
    (inst << 39 >> 59) as u8
}
fn operand_rs3(inst: RvInst) -> u8 {
    (inst << 32 >> 59) as u8
}
fn operand_aq(inst: RvInst) -> u8 {
    (inst << 37 >> 63) as u8
}
fn operand_rl(inst: RvInst) -> u8 {
    (inst << 38 >> 63) as u8
}
fn operand_pred(inst: RvInst) -> u8 {
    (inst << 36 >> 60) as u8
}
fn operand_succ(inst: RvInst) -> u8 {
    (inst << 40 >> 60) as u8
}
fn operand_rm(inst: RvInst) -> u8 {
    (inst << 49 >> 61) as u8
}
fn operand_shamt5(inst: RvInst) -> i32 {
    (inst << 39 >> 59) as i32
}
fn operand_shamt6(inst: RvInst) -> i32 {
    (inst << 38 >> 58) as i32
}
fn operand_shamt7(inst: RvInst) -> i32 {
    (inst << 37 >> 57) as i32
}
fn operand_crdq(inst: RvInst) -> u8 {
    (inst << 59 >> 61) as u8
}
fn operand_crs1q(inst: RvInst) -> u8 {
    (inst << 54 >> 61) as u8
}
fn operand_crs1rdq(inst: RvInst) -> u8 {
    (inst << 54 >> 61) as u8
}
fn operand_crs2q(inst: RvInst) -> u8 {
    (inst << 59 >> 61) as u8
}
fn operand_crd(inst: RvInst) -> u8 {
    (inst << 52 >> 59) as u8
}
fn operand_crs1(inst: RvInst) -> u8 {
    (inst << 52 >> 59) as u8
}
fn operand_crs1rd(inst: RvInst) -> u8 {
    (inst << 52 >> 59) as u8
}
fn operand_crs2(inst: RvInst) -> u8 {
    (inst << 57 >> 59) as u8
}
fn operand_cimmsh5(inst: RvInst) -> i32 {
    (inst << 57 >> 59) as i32
}
fn operand_csr12(inst: RvInst) -> i32 {
    (inst << 32 >> 52) as i32
}
fn operand_imm12(inst: RvInst) -> i32 {
    ((inst as i64) << 32 >> 52) as i32
}
fn operand_imm20(inst: RvInst) -> i32 {
    (((inst as i64) << 32 >> 44) << 12) as i32
}
fn operand_jimm20(inst: RvInst) -> i32 {
    ((((inst as i64) << 32 >> 63) << 20) as u64
        | (inst << 33 >> 54) << 1
        | (inst << 43 >> 63) << 11
        | (inst << 44 >> 56) << 12) as i32
}
fn operand_simm12(inst: RvInst) -> i32 {
    ((((inst as i64) << 32 >> 57) << 5) as u64 | inst << 52 >> 59) as i32
}
fn operand_sbimm12(inst: RvInst) -> i32 {
    ((((inst as i64) << 32 >> 63) << 12) as u64
        | (inst << 33 >> 58) << 5
        | (inst << 52 >> 60) << 1
        | (inst << 56 >> 63) << 11) as i32
}
fn operand_cimmsh6(inst: RvInst) -> i32 {
    ((inst << 51 >> 63) << 5 | inst << 57 >> 59) as i32
}
fn operand_cimmi(inst: RvInst) -> i32 {
    ((((inst as i64) << 51 >> 63) << 5) as u64 | inst << 57 >> 59) as i32
}
fn operand_cimmui(inst: RvInst) -> i32 {
    ((((inst as i64) << 51 >> 63) << 17) as u64 | (inst << 57 >> 59) << 12) as i32
}
fn operand_cimmlwsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 63) << 5 | (inst << 57 >> 61) << 2 | (inst << 60 >> 62) << 6) as i32
}
fn operand_cimmldsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 63) << 5 | (inst << 57 >> 62) << 3 | (inst << 59 >> 61) << 6) as i32
}
fn operand_cimmlqsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 63) << 5 | (inst << 57 >> 63) << 4 | (inst << 58 >> 60) << 6) as i32
}
fn operand_cimm16sp(inst: RvInst) -> i32 {
    ((((inst as i64) << 51 >> 63) << 9) as u64
        | (inst << 57 >> 63) << 4
        | (inst << 58 >> 63) << 6
        | (inst << 59 >> 62) << 7
        | (inst << 61 >> 63) << 5) as i32
}
fn operand_cimmj(inst: RvInst) -> i32 {
    ((((inst as i64) << 51 >> 63) << 11) as u64
        | (inst << 52 >> 63) << 4
        | (inst << 53 >> 62) << 8
        | (inst << 55 >> 63) << 10
        | (inst << 56 >> 63) << 6
        | (inst << 57 >> 63) << 7
        | (inst << 58 >> 61) << 1
        | (inst << 61 >> 63) << 5) as i32
}
fn operand_cimmb(inst: RvInst) -> i32 {
    ((((inst as i64) << 51 >> 63) << 8) as u64
        | (inst << 52 >> 62) << 3
        | (inst << 57 >> 62) << 6
        | (inst << 59 >> 62) << 1
        | (inst << 61 >> 63) << 5) as i32
}
fn operand_cimmswsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 60) << 2 | (inst << 55 >> 62) << 6) as i32
}
fn operand_cimmsdsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 61) << 3 | (inst << 54 >> 61) << 6) as i32
}
fn operand_cimmsqsp(inst: RvInst) -> i32 {
    ((inst << 51 >> 62) << 4 | (inst << 53 >> 60) << 6) as i32
}
fn operand_cimm4spn(inst: RvInst) -> i32 {
    ((inst << 51 >> 62) << 4
        | (inst << 53 >> 60) << 6
        | (inst << 57 >> 63) << 2
        | (inst << 58 >> 63) << 3) as i32
}
fn operand_cimmw(inst: RvInst) -> i32 {
    ((inst << 51 >> 61) << 3 | (inst << 57 >> 63) << 2 | (inst << 58 >> 63) << 6) as i32
}
fn operand_cimmd(inst: RvInst) -> i32 {
    ((inst << 51 >> 61) << 3 | (inst << 57 >> 62) << 6) as i32
}
fn operand_cimmq(inst: RvInst) -> i32 {
    ((inst << 51 >> 62) << 4 | (inst << 53 >> 63) << 8 | (inst << 57 >> 62) << 6) as i32
}

fn decode_inst_operands(dec: &mut RvDecode) {
    let inst = dec.inst;
    dec.codec = OPCODE_DATA[dec.op as usize].codec;
    match dec.codec {
        RV_CODEC_NONE => {
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.rd = dec.rs1;
            dec.imm = 0;
        }
        RV_CODEC_U => {
            dec.rd = operand_rd(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.imm = operand_imm20(inst);
        }
        3 => {
            dec.rd = operand_rd(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.imm = operand_jimm20(inst);
        }
        4 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_imm12(inst);
        }
        5 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_shamt5(inst);
        }
        6 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_shamt6(inst);
        }
        7 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_shamt7(inst);
        }
        8 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_csr12(inst);
        }
        9 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.imm = operand_simm12(inst);
        }
        10 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.imm = operand_sbimm12(inst);
        }
        11 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.imm = 0;
        }
        12 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.rm = operand_rm(inst);
            dec.imm = 0;
        }
        13 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.rs3 = operand_rs3(inst);
            dec.imm = 0;
            dec.rm = operand_rm(inst);
        }
        14 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = operand_rs2(inst);
            dec.imm = 0;
            dec.aq = operand_aq(inst);
            dec.rl = operand_rl(inst);
        }
        15 => {
            dec.rd = operand_rd(inst);
            dec.rs1 = operand_rs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = 0;
            dec.aq = operand_aq(inst);
            dec.rl = operand_rl(inst);
        }
        16 => {
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.rd = dec.rs1;
            dec.pred = operand_pred(inst);
            dec.succ = operand_succ(inst);
            dec.imm = 0;
        }
        17 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = operand_crs1q(inst).wrapping_add(8);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmb(inst);
        }
        18 => {
            dec.rs1 = operand_crs1rdq(inst).wrapping_add(8);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmi(inst);
        }
        19 => {
            dec.rs1 = operand_crs1rdq(inst).wrapping_add(8);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmsh5(inst);
        }
        20 => {
            dec.rs1 = operand_crs1rdq(inst).wrapping_add(8);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmsh6(inst);
        }
        21 => {
            dec.rs1 = operand_crs1rd(inst);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmi(inst);
        }
        22 => {
            dec.rs1 = operand_crs1rd(inst);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmsh5(inst);
        }
        23 => {
            dec.rs1 = operand_crs1rd(inst);
            dec.rd = dec.rs1;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmsh6(inst);
        }
        24 => {
            dec.rd = RV_IREG_SP;
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimm16sp(inst);
        }
        25 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmlwsp(inst);
        }
        26 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmldsp(inst);
        }
        27 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmlqsp(inst);
        }
        28 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = RV_IREG_ZERO;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmi(inst);
        }
        29 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = RV_IREG_ZERO;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmui(inst);
        }
        30 => {
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.rd = dec.rs1;
            dec.imm = 0;
        }
        31 => {
            dec.rd = operand_crdq(inst).wrapping_add(8);
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimm4spn(inst);
        }
        32 => {
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.rd = dec.rs1;
            dec.imm = operand_cimmj(inst);
        }
        33 => {
            dec.rd = RV_IREG_RA;
            dec.rs2 = RV_IREG_ZERO;
            dec.rs1 = dec.rs2;
            dec.imm = operand_cimmj(inst);
        }
        34 => {
            dec.rd = operand_crdq(inst).wrapping_add(8);
            dec.rs1 = operand_crs1q(inst).wrapping_add(8);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmw(inst);
        }
        35 => {
            dec.rd = operand_crdq(inst).wrapping_add(8);
            dec.rs1 = operand_crs1q(inst).wrapping_add(8);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmd(inst);
        }
        36 => {
            dec.rd = operand_crdq(inst).wrapping_add(8);
            dec.rs1 = operand_crs1q(inst).wrapping_add(8);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = operand_cimmq(inst);
        }
        37 => {
            dec.rs1 = operand_crs1rd(inst);
            dec.rd = dec.rs1;
            dec.rs2 = operand_crs2(inst);
            dec.imm = 0;
        }
        38 => {
            dec.rd = operand_crd(inst);
            dec.rs1 = operand_crs2(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = 0;
        }
        39 => {
            dec.rd = RV_IREG_RA;
            dec.rs1 = operand_crs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = 0;
        }
        40 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = operand_crs1(inst);
            dec.rs2 = RV_IREG_ZERO;
            dec.imm = 0;
        }
        41 => {
            dec.rs1 = operand_crs1rdq(inst).wrapping_add(8);
            dec.rd = dec.rs1;
            dec.rs2 = operand_crs2q(inst).wrapping_add(8);
            dec.imm = 0;
        }
        42 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = (operand_crs1q(inst)).wrapping_add(8);
            dec.rs2 = (operand_crs2q(inst)).wrapping_add(8);
            dec.imm = operand_cimmw(inst);
        }
        43 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = (operand_crs1q(inst)).wrapping_add(8);
            dec.rs2 = (operand_crs2q(inst)).wrapping_add(8);
            dec.imm = operand_cimmd(inst);
        }
        44 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = (operand_crs1q(inst)).wrapping_add(8);
            dec.rs2 = (operand_crs2q(inst)).wrapping_add(8);
            dec.imm = operand_cimmq(inst);
        }
        45 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = operand_crs2(inst);
            dec.imm = operand_cimmswsp(inst);
        }
        46 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = operand_crs2(inst);
            dec.imm = operand_cimmsdsp(inst);
        }
        47 => {
            dec.rd = RV_IREG_ZERO;
            dec.rs1 = RV_IREG_SP;
            dec.rs2 = operand_crs2(inst);
            dec.imm = operand_cimmsqsp(inst);
        }
        _ => {}
    };
}

fn decode_inst_decompress(dec: &mut RvDecode, isa: RvIsa) {
    if let Some(decomp_op) = match isa {
        RvIsa::Rv32 => OPCODE_DATA[dec.op as usize].decomp_rv32,
        RvIsa::Rv64 => OPCODE_DATA[dec.op as usize].decomp_rv64,
        RvIsa::Rv128 => OPCODE_DATA[dec.op as usize].decomp_rv128,
    } {
        if OPCODE_DATA[dec.op as usize].decomp_data == RvCdImmediate::Nz && dec.imm == 0 {
            dec.op = RvOp::Illegal;
        } else {
            dec.op = decomp_op;
            dec.codec = OPCODE_DATA[decomp_op as usize].codec as u8;
        }
    }
}

fn check_constraints(dec: &RvDecode, constraints: &[RvcConstraint]) -> bool {
    let imm = dec.imm;
    let rd = dec.rd;
    let rs1 = dec.rs1;
    let rs2 = dec.rs2;
    for c in constraints {
        match *c {
            RVC_RD_EQ_RA => {
                if !(rd == 1) {
                    return false;
                }
            }
            RVC_RD_EQ_X0 => {
                if rd != 0 {
                    return false;
                }
            }
            RVC_RS1_EQ_X0 => {
                if rs1 != 0 {
                    return false;
                }
            }
            RVC_RS2_EQ_X0 => {
                if rs2 != 0 {
                    return false;
                }
            }
            RVC_RS2_EQ_RS1 => {
                if rs2 != rs1 {
                    return false;
                }
            }
            RVC_IMM_EQ_RA => {
                if rs1 != 1 {
                    return false;
                }
            }
            RVC_IMM_EQ_ZERO => {
                if imm != 0 {
                    return false;
                }
            }
            RVC_IMM_EQ_N1 => {
                if imm != -1 {
                    return false;
                }
            }
            RVC_IMM_EQ_P1 => {
                if imm != 1 {
                    return false;
                }
            }
            RVC_CSR_EQ_0X001 => {
                if imm != 0x1 {
                    return false;
                }
            }
            RVC_CSR_EQ_0X002 => {
                if imm != 0x2 {
                    return false;
                }
            }
            RVC_CSR_EQ_0X003 => {
                if imm != 0x3 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC00 => {
                if imm != 0xc00 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC01 => {
                if imm != 0xc01 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC02 => {
                if imm != 0xc02 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC80 => {
                if imm != 0xc80 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC81 => {
                if imm != 0xc81 {
                    return false;
                }
            }
            RVC_CSR_EQ_0XC82 => {
                if imm != 0xc82 {
                    return false;
                }
            }
            _ => {}
        }
    }

    return true;
}

fn decode_inst_lift_pseudo(dec: &mut RvDecode) {
    for comp_data in OPCODE_DATA[dec.op as usize].pseudo {
        if check_constraints(dec, comp_data.constraints) {
            dec.op = comp_data.op;
            dec.codec = OPCODE_DATA[dec.op as usize].codec;
            return;
        }
    }
}

fn decode_inst_format(_tab: usize, dec: &RvDecode) {
    match inst_length(dec.inst) {
        2 => {
            print!("{:04x}      ", dec.inst & 0xffff);
        }
        4 => {
            print!("{:08x}  ", dec.inst & 0xffff_ffff);
        }
        6 => {
            print!("{:012x}", dec.inst & 0xffff_ffff_ffff);
        }
        _ => {
            print!("{:016x}", dec.inst);
        }
    }

    for fmt in OPCODE_DATA[dec.op as usize].format {
        match fmt {
            b'O' => {
                print!("{}", OPCODE_DATA[dec.op as usize].name);
            }
            b'(' => {
                print!("(");
            }
            b',' => {
                print!(",");
            }
            b')' => {
                print!(")");
            }
            b'0' => {
                print!("{}", RV_IREG_NAME_SYM[dec.rd as usize]);
            }
            b'1' => {
                print!("{}", RV_IREG_NAME_SYM[dec.rs1 as usize]);
            }
            b'2' => {
                print!("{}", (RV_IREG_NAME_SYM[dec.rs2 as usize]));
            }
            b'3' => {
                print!("{}", (RV_FREG_NAME_SYM[dec.rd as usize]));
            }
            b'4' => {
                print!("{}", (RV_FREG_NAME_SYM[dec.rs1 as usize]));
            }
            b'5' => {
                print!("{}", (RV_FREG_NAME_SYM[dec.rs2 as usize]));
            }
            b'6' => {
                print!("{}", (RV_FREG_NAME_SYM[dec.rs3 as usize]));
            }
            b'7' => {
                print!("{}", dec.rs1);
            }
            b'i' => {
                print!("{}", dec.imm);
            }
            b'o' => {
                print!("{}", dec.imm);
                print!("   ");
                print!("# 0x{:x}", dec.pc.wrapping_add(dec.imm as u64));
            }
            b'c' => match csr_name(dec.imm & 0xfff) {
                Some(name) => print!("{}", name),
                None => print!("{:03x}", dec.imm & 0xfff),
            },
            b'r' => match dec.rm {
                0 => {
                    print!("rne");
                }
                1 => {
                    print!("rtz");
                }
                2 => {
                    print!("rdn");
                }
                3 => {
                    print!("rup");
                }
                4 => {
                    print!("rmm");
                }
                7 => {
                    print!("dyn");
                }
                _ => {
                    print!("inv");
                }
            },
            b'p' => {
                if dec.pred & RV_FENCE_I != 0 {
                    print!("i");
                }
                if dec.pred & RV_FENCE_O != 0 {
                    print!("o");
                }
                if dec.pred & RV_FENCE_R != 0 {
                    print!("r");
                }
                if dec.pred & RV_FENCE_W != 0 {
                    print!("w");
                }
            }
            b's' => {
                if dec.succ & RV_FENCE_I != 0 {
                    print!("i");
                }
                if dec.succ & RV_FENCE_O != 0 {
                    print!("o");
                }
                if dec.succ & RV_FENCE_R != 0 {
                    print!("r");
                }
                if dec.succ & RV_FENCE_W != 0 {
                    print!("w");
                }
            }
            b'\t' => {
                print!("\t");
            }
            b'A' => {
                if dec.aq != 0 {
                    print!(".aq");
                }
            }
            b'R' => {
                if dec.rl != 0 {
                    print!(".rl");
                }
            }
            _ => {}
        }
    }
}

fn inst_length(inst: RvInst) -> usize {
    return (if inst & 0o3 != 0o3 {
        2
    } else if inst & 0o34 != 0o34 {
        4
    } else if inst & 0o77 == 0o37 {
        6
    } else if inst & 0o177 == 0o77 {
        8
    } else {
        0
    }) as usize;
}

#[allow(dead_code)]
pub fn inst_fetch(data: &[u8]) -> (RvInst, usize) {
    let mut inst: RvInst = (data[1] as RvInst) << 8 | data[0] as RvInst;

    let length = inst_length(inst);
    if length >= 8 {
        inst |= (data[7] as RvInst) << 56 | (data[6] as RvInst) << 48;
    }
    if length >= 6 {
        inst |= (data[5] as RvInst) << 40 | (data[4] as RvInst) << 32;
    }

    if length >= 4 {
        inst |= (data[3] as RvInst) << 24 | (data[2] as RvInst) << 16;
    }

    (inst, length)
}

pub fn disasm_inst(isa: RvIsa, pc: u64, inst: RvInst) {
    let mut dec = RvDecode {
        pc,
        inst,
        imm: 0,
        op: RvOp::Illegal,
        codec: 0,
        rd: 0,
        rs1: 0,
        rs2: 0,
        rs3: 0,
        rm: 0,
        pred: 0,
        succ: 0,
        aq: 0,
        rl: 0,
    };
    decode_inst_opcode(&mut dec, isa);
    decode_inst_operands(&mut dec);
    decode_inst_decompress(&mut dec, isa);
    decode_inst_lift_pseudo(&mut dec);
    decode_inst_format(32, &dec);
}
