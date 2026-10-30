//! RSA-1024 raw private-key Known-Answer Test on the SCE PKE.
//!
//! Purpose: prove the PKE block (APB SFRs + SCE RAM window + PKE/MIMM datapath) is wired
//! up correctly in a Verilator sim.
//!
//! Derived from https://github.com/crossbar-inc/daric-sdk (`daric-sdk/drivers/Src/sce/pke_rsa.c`:
//! `sce_Sysinit_Pke` -> `bn_init(advance_mode=true)` -> `bn_expo_mod_sn`) by
//! qwen3.8-flash-next-unsloth-ud-iq4_xs + omp harness.
//!
//! The test computes `sig = m^d mod n` on hardware for a freshly generated RSA-1024 key
//! and compares against a known answer precomputed with Python's `pow(m, d, n)` (an
//! independent trusted implementation). `sig^e == m` was verified host-side; see
//! `KAT_VECTORS` below for the generation recipe.
//!
//! Deviations from the C driver, all deliberate and noted at the call site:
//!   - The `bn_mult_mod_EX()` 6144-bit dummy modmul "cache flush" is skipped. It exists to work around a
//!     suspected MIMM cache-coherency erratum that only matters when moduli are switched mid-run; a fresh sim
//!     boot has a cold MIMM and this KAT uses exactly one modulus. Re-add it if the sim hangs or mismatches.
//!   - No `assert!` on operand sanity or security hardening.

// SCE global SFRs (utralib `sce_glbsfr`, base 0x4002_8000) and PKE SFRs (`pke`, 0x4002_c000)
use utra::pke as pkereg;
use utra::sce_glbsfr as glbreg;
use utralib::*;

use crate::boot::setup;

/// SCE internal RAM window base (C: `SCE_RAM_ADDR = SCE_BASE_ADDR = 0x4002_0000`; the same
/// window the combohash demo writes via `HW_SEG_LKEY_MEM`); PKE segment byte offsets per
/// `auth.h`: PCON = +0x800 (1 KiB, aliases PKB), PIB = +0xC00 (1 KiB), POB = +0x1400 (1 KiB).
const SCE_RAM: usize = utralib::HW_SEG_LKEY_MEM;
const SEG_PCON: usize = 0x800;
const SEG_PIB: usize = 0xC00;
const SEG_POB: usize = 0x1400;

// C: PIR_RSAINIT = 0x11, PIR_RSAMM = 0x12, PIR_RSAME = 0x13 (auth.h enum)
const IR_RSAINIT: u32 = 0x11;
const IR_RSAME: u32 = 0x13;

// C: PKE_MIMMCR_CONFIG = 0x103 (4 modular multipliers)
const MIMMCR_4MUL: u32 = 0x103;

// C: SUBCLKEN bits: SDMA=1<<0, PKE=1<<2 (auth.h). PKE clock is bit 2; the C sysinit also
// enables SDMA, mirrored for fidelity.
const SUBEN_SDMA: u32 = 1 << 0;
const SUBEN_PKE: u32 = 1 << 2;

// C: SCE_RAM_CLEAR_BUSY = 1<<8 in REG_SCE_CTRL_BUSYSR == SFR_SRBUSY
const RAM_CLEAR_BUSY: u32 = 1 << 8;

const NLEN_WORDS: usize = 32; // 1024 bits / 32
const MODULO_LEN: u32 = 1024;

/// Big-endian hex -> little-endian u32 words (word 0 = least-significant), matching the
/// layout `bnu_memcpy_u32` writes into SCE RAM with `RSA_LITTLE_ENDIAN` and OPTLTX = 0.
const fn be_hex_to_le_words(hex: &[u8; 256]) -> [u32; NLEN_WORDS] {
    const fn nib(c: u8) -> u32 {
        match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'f' => (c - b'a' + 10) as u32,
            b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => panic!("bad hex digit"),
        }
    }
    let mut w = [0u32; NLEN_WORDS];
    let mut i = 0; // BE word index (0 = most significant)
    while i < NLEN_WORDS {
        let mut word = 0u32;
        let mut b = 0;
        while b < 4 {
            let hi = nib(hex[i * 8 + b * 2]);
            let lo = nib(hex[i * 8 + b * 2 + 1]);
            word = (word << 8) | (hi << 4) | lo;
            b += 1;
        }
        w[NLEN_WORDS - 1 - i] = word;
        i += 1;
    }
    w
}

/// Known-answer vectors.
///
/// Generation (documented so the vectors are reproducible/verifiable):
///   rng = random.Random(0x5eed_2026_1030); 512-bit Miller-Rabin primes p,q (40 rounds);
///   n = p*q (1024 bits), e = 65537, d = pow(e, -1, (p-1)*(q-1));
///   m = int.from_bytes(b"primordial PKE wiring KAT: RSA-1024 raw m^d mod N" * 2, "big");
///   sig = pow(m, d, n).   Host-checked: pow(sig, e, n) == m.
/// m is the ASCII pattern repeated twice (783 bits, trivially < n); the 1024-bit operand
/// slots and the full-length exponent d exercise the whole datapath.
struct Kat {
    n: [u32; NLEN_WORDS],
    d: [u32; NLEN_WORDS],
    m: [u32; NLEN_WORDS],
    expect: [u32; NLEN_WORDS],
}

const KAT_VECTORS: Kat = {
    const N_HEX: &[u8; 256] = b"a9d516433f4198f3d8e5a2081adb5e04f1c3460a532a070d08c7ebbc938720ea\
ea97997251670d61e02df16d2e9cd568ec0ad9f6b3888276d7dde8eda59886dab3564e8ae4529f659bc432dd161dbb0b\
9633cea9b40f75eb3fd53bb0379c7841d72ee1ceaaf8ab9cdb873b3b3f316ac517c3223a32aef40269a4d67ae935ab67";
    const D_HEX: &[u8; 256] = b"126c17e721847717f46e2c5e5eb36f0670473360c956a4bf2835af7f9c2d2b47\
f03c939b8ff2f2c3ac32af9e9d50cbe17ec14a957532189cd59bafb99f9de4fac4bb10546f17aab970c2758f747d0beb\
57fb844da749c4c24739152d1eef9f982861b420f5720d862e894144471f8b1f6937c1c6e1f26692de7c1a46f8d75101";
    const M_HEX: &[u8; 256] = b"0000000000000000000000000000000000000000000000000000000000007072\
696d6f726469616c20504b4520776972696e67204b41543a205253412d3130323420726177206d5e64206d6f64204e\
7072696d6f726469616c20504b4520776972696e67204b41543a205253412d3130323420726177206d5e64206d6f64204e";
    const S_HEX: &[u8; 256] = b"9071790ebabe488da201221b434be78de74a45b13fb62e63747818965977ceff\
d8754e287963f849e981d57310c9137a1f96833f04abf0c88ee79a9ada61f6988dc819a3e91c559945343443438c2567\
6cac09ed0b01278bb7b9c9cf67558b0260953ade581c0e924f1df1ca2ae7b17e57f6330e878e7c6a22222ebd7d045296";
    Kat {
        n: be_hex_to_le_words(N_HEX),
        d: be_hex_to_le_words(D_HEX),
        m: be_hex_to_le_words(M_HEX),
        expect: be_hex_to_le_words(S_HEX),
    }
};

fn pke_wait_done(pke: &mut CSR<u32>) {
    // C: SCE_PKE_DONE(): while (!(FR & 0x01)); then write-1-to-clear.
    // Bounded for sim bring-up: on timeout dump the raw flags and return.
    let mut spins: u32 = 0;
    loop {
        if pke.rf(pkereg::SFR_FR_MFSM_DONE) != 0 {
            break;
        }
        spins = spins.wrapping_add(1);
        if spins == 0 {
            crate::println!(
                "  pke_wait_done TIMEOUT: FR={:08x} CRFUNC={:08x} BUSY={:08x}",
                pke.r(pkereg::SFR_FR),
                pke.r(pkereg::SFR_CRFUNC),
                pke.r(pkereg::SFR_SRMFSM)
            );
            return;
        }
    }
    pke.rmwf(pkereg::SFR_FR_MFSM_DONE, 1);
}

/// C: sce_Sysinit_Pke() - clocks on, core reset strobe, reconfigure, RAM zero-fill.
/// Note `sce_glbsfr` does not expose SCEMODE (reg 0); it resets to 0 (normal) and we do not
/// touch it, so the C `SET_SECMODE_NORMAL()` is a no-op relative to us.
fn glb_wait_idle(glb: &mut CSR<u32>, tag: &str) {
    let mut spins: u32 = 0;
    while (glb.r(glbreg::SFR_SRBUSY) & RAM_CLEAR_BUSY) != 0 {
        spins += 1;
        if spins >= (1 << 20) {
            // stuck: dump raw state and bail out rather than hang
            crate::println!(
                "  [{}] SRBUSY stuck: {:08x} FRDONE {:08x} FRERR {:08x}",
                tag,
                glb.r(glbreg::SFR_SRBUSY),
                glb.r(glbreg::SFR_FRDONE),
                glb.r(glbreg::SFR_FRERR)
            );
            return;
        }
    }
    crate::println!("  [{}] idle after {} spins", tag, spins);
}

fn sce_sysinit_pke(glb: &mut CSR<u32>) {
    glb.wfo(glbreg::SFR_SUBEN_CR_SUBEN, SUBEN_SDMA | SUBEN_PKE);
    crate::println!("  suben = {:08x}", glb.r(glbreg::SFR_SUBEN));
    // C: SCE_CLR_RESET_RAM() = RESETRAM 0x5A: pulse-resets the algorithm cores and clears
    // all scectrl configurations (including SUBEN itself).
    glb.wo(glbreg::SFR_ARCLR, 0x5a);
    glb_wait_idle(glb, "reset-ram 5a");
    // reconfigure after the reset strobe
    glb.wfo(glbreg::SFR_SUBEN_CR_SUBEN, SUBEN_SDMA | SUBEN_PKE);
    // C: SCE_CLR_RAM() = RESETRAM 0xA5: hardware zero-fill of the whole SCE RAM.
    glb.wo(glbreg::SFR_ARCLR, 0xa5);
    glb_wait_idle(glb, "clr-ram a5");
}

/// C: bn_init(ctx, n, 32, advance_mode=true) for a 1024-bit odd n.
/// Programs OPTNW + MIMMCR, copies N and the HCAL operand H = 2^1024 into the PCON
/// segment, then runs RSAINIT (latches N, N', H into PKE-internal storage). The
/// `bn_mult_mod_EX()` + MIMMCR=0x100 cache flush is skipped (cold MIMM, single modulus -
/// see module doc); MIMMCR=0x103 (4 multipliers) matches the C final state.
fn pke_rsa_init(pke: &mut CSR<u32>, n: &[u32; NLEN_WORDS]) {
    pke.wo(pkereg::SFR_MIMMCR, MIMMCR_4MUL);

    pke.wfo(pkereg::SFR_OPTNW_SFR_OPTNW, MODULO_LEN);
    // Segment pointer SFRs select an offset within each PKE segment (the C driver always
    // writes 0 in the RSA path: "REG_SCE_PKE_PCON/PIB0/PKB/POB = 0" selects the segment
    // base). The segment base addresses themselves are hardwired per auth.h: PCON ->
    // SCE_RAM+0x800, PIB -> +0xC00, PKB -> +0x800 (aliases PCON), POB -> +0x1400 - which is
    // also exactly where the C driver's CPU-side operand copies land.
    pke.wo(pkereg::SFR_SEGPTR_PTRID_PCON, 0x0); // C: REG_SCE_PKE_PCON = 0

    // N occupies the full 1 KiB PCON segment; the H block lands immediately after it
    // (AddrH = PCON + ((nzw+1) & !1) = PCON + 32 words), inside PIB's first 34 words -
    // exactly what the C driver writes before RSAINIT consumes both.
    let pcon = unsafe { core::slice::from_raw_parts_mut((SCE_RAM + SEG_PCON) as *mut u32, 0x400 / 4) };
    // C: bnu_memcpy_u32(ADDR_PKE_SEG_PCON, n, n_len)
    pcon[..NLEN_WORDS].copy_from_slice(n);

    // C: for i in 0..(modulo_len/32): AddrH[i] = 0; AddrH[i] = 1 << (modulo_len % 32);
    //    AddrH[i+1] = 0;   -> 32 zero words, then word 1, then word 0 (HW reads 34 words).
    let addr_h = NLEN_WORDS; // PCON word 32
    for w in pcon[addr_h..addr_h + (MODULO_LEN as usize / 32)].iter_mut() {
        *w = 0;
    }
    pcon[addr_h + (MODULO_LEN as usize / 32)] = 1u32 << (MODULO_LEN % 32);
    pcon[addr_h + (MODULO_LEN as usize / 32) + 1] = 0;

    // C: SCE_PKE_FUNC(PIR_RSAINIT); SCE_PKE_AR(); SCE_PKE_DONE();
    pke.wo(pkereg::SFR_CRFUNC, IR_RSAINIT);
    pke.wo(pkereg::SFR_FR, 0xf); // write-1-to-clear stale completion flags
    pke.wo(pkereg::SFR_AR2, 0x5a);
    pke_wait_done(pke);
}

/// C: bn_expo_mod_sn(ctx, m, d, 32, 32, result): result = m^d mod n (LE word buffers).
/// The 1024-bit modulus keeps `pack_len == 0` (modulo_len % 256 == 0), so the C repack
/// tail is not reached and the output is a plain 32-word LE copy from POB.
fn pke_modexp(pke: &mut CSR<u32>, m: &[u32; NLEN_WORDS], d: &[u32; NLEN_WORDS], out: &mut [u32; NLEN_WORDS]) {
    // C: expo_len = bnu_get_msb_le(d, ylen) + 1 (d is 1021 bits here)
    let expo_len = {
        let mut top = NLEN_WORDS - 1;
        while d[top] == 0 {
            top -= 1;
        }
        (top * 32 + (31 - d[top].leading_zeros() as usize)) as u32 + 1
    };
    pke.wfo(pkereg::SFR_OPTEW_SFR_OPTEW, expo_len);

    // C: memset PIB 2048B, PKB 1024B, POB 1024B. PKB aliases PCON; C clears N there too -
    // safe because RSAINIT already latched N into PKE-internal storage.
    let pib = unsafe { core::slice::from_raw_parts_mut((SCE_RAM + SEG_PIB) as *mut u32, 2048 / 4) };
    pib.fill(0);
    let pkb = unsafe { core::slice::from_raw_parts_mut((SCE_RAM + SEG_PCON) as *mut u32, 1024 / 4) };
    pkb.fill(0);
    let pob = unsafe { core::slice::from_raw_parts_mut((SCE_RAM + SEG_POB) as *mut u32, 1024 / 4) };
    pob.fill(0);

    // C: X -> PIB, REG_PIB0 = 0; Y -> PKB, REG_PKB = 0; REG_POB = 0.
    pib[..NLEN_WORDS].copy_from_slice(m);
    pke.wo(pkereg::SFR_SEGPTR_PTRID_PIB0, 0x0);
    pkb[..NLEN_WORDS].copy_from_slice(d);
    pke.wo(pkereg::SFR_SEGPTR_PTRID_PKB, 0x0);
    pke.wo(pkereg::SFR_SEGPTR_PTRID_POB, 0x0);

    // C: SCE_PKE_FUNC(PIR_RSAME); SCE_PKE_AR(); SCE_PKE_DONE();
    pke.wo(pkereg::SFR_CRFUNC, IR_RSAME);
    pke.wo(pkereg::SFR_FR, 0xf);
    pke.wo(pkereg::SFR_AR2, 0x5a);
    pke_wait_done(pke);

    let pob = unsafe { core::slice::from_raw_parts((SCE_RAM + SEG_POB) as *const u32, NLEN_WORDS) };
    out.copy_from_slice(pob);
}

pub fn sce_rsa() {
    crate::println!("sce_rsa(): init");
    setup();

    let mut glb = CSR::new(glbreg::HW_SCE_GLBSFR_BASE as *mut u32);
    let mut pke = CSR::new(pkereg::HW_PKE_BASE as *mut u32);

    crate::println!("sce_rsa(): sysinit");
    sce_sysinit_pke(&mut glb);

    // Endianness/mask defaults we rely on (reset values on a fresh sim; the C RSA path
    // never programs them either): OPTLTX = 0 (little-endian words), OPTMASK = 0.
    assert_eq!(pke.r(pkereg::SFR_OPTLTX), 0, "OPTLTX should reset to 0 (little-endian)");
    assert_eq!(pke.r(pkereg::SFR_OPTMASK), 0, "OPTMASK should reset to 0");

    crate::println!("sce_rsa(): rsa init");
    pke_rsa_init(&mut pke, &KAT_VECTORS.n);

    crate::println!("sce_rsa(): KAT starting");
    let mut result = [0u32; NLEN_WORDS];
    pke_modexp(&mut pke, &KAT_VECTORS.m, &KAT_VECTORS.d, &mut result);

    if result == KAT_VECTORS.expect {
        crate::println!("RSA-1024 m^d mod N KAT passed");
    } else {
        crate::println!("RSA-1024 m^d mod N KAT FAILED");
        crate::println!("  [word]: got / expected (LE word order)");
        for i in 0..NLEN_WORDS {
            if result[i] != KAT_VECTORS.expect[i] {
                crate::println!("  [{:02}]: {:08x} / {:08x}", i, result[i], KAT_VECTORS.expect[i]);
            }
        }
    }
}
