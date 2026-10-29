use bao1x_api::signatures::SIGBLOCK_LEN;
use digest::Digest;
use sha2_bao1x::Sha512;
use utralib::*;

const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = crate::asm::RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

fn setup() {
    const STATICS_LOC: usize = bao1x_api::BOOT0_START + SIGBLOCK_LEN;

    let statics_in_rom: &bao1x_api::StaticsInRom =
        unsafe { (STATICS_LOC as *const bao1x_api::StaticsInRom).as_ref().unwrap() };
    assert!(statics_in_rom.version == bao1x_api::STATICS_IN_ROM_VERSION, "Can't find valid statics table");

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    // Safety: only safe if the values computed by the loader are correct.
    unsafe {
        let data_ptr = statics_in_rom.data_origin as *mut u32;
        for i in 0..statics_in_rom.data_size_bytes as usize / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for &(offset, data) in &statics_in_rom.poke_table[..statics_in_rom.valid_pokes as usize] {
            data_ptr
                .add(u16::from_le_bytes(offset) as usize / size_of::<u32>())
                .write_volatile(u32::from_le_bytes(data));
        }
    }
    unsafe {
        crate::ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}

/// This function loads all the round constants into the combohasher's local memory.
pub fn init_hash() {
    use bao1x_api::sce::combohash::*;
    // safety: this one is a little less clear from the register set extraction. But in the case of
    // system initialization, the SCE uses its entire RAM range (10kiB worth) as a single buffer, so
    // none of the buffer boundaries are respected. Thus we set the length of the segment to 10kiB
    // solely because as hardware designers, we know this is what's there. You can see the size of
    // the SCERAM's block definition via its RBIST wrapper here:
    // https://github.com/baochip/baochip-1x/blob/96ba390759ba361e50e57bd21f02c806ddafc4ff/rtl/modules/soc_coresub/rtl/soc_coresub.sv#L1018
    let sce_mem = unsafe {
        core::slice::from_raw_parts_mut(utralib::HW_SEG_LKEY_MEM as *mut u32, 10 * 1024 / size_of::<u32>())
    };
    #[rustfmt::skip]
    let constants =
        SHA256_H.iter().chain(
        SHA256_K.iter().chain(
        SHA512_H.iter().chain(
        SHA512_K.iter().chain(
        BLK2S_H.iter().chain(
        BLK2B_H.iter().chain(
        BLK2_X.iter().chain(
        BLK3_H.iter().chain(
        BLK3_X.iter().chain(
        RIPMD_H.iter().chain(
        RIPMD_K.iter().chain(
        RIPMD_X.iter().chain(
        RAMSEG_SHA3.iter()
    ))))))))))));
    for (dst, &src) in sce_mem.iter_mut().zip(constants) {
        *dst = src;
    }
    let mut combo_hash = CSR::new(utra::combohash::HW_COMBOHASH_BASE as *mut u32);
    combo_hash.wo(utra::combohash::SFR_OPT3, 0); // u32 big-endian constant load
    combo_hash.wfo(utra::combohash::SFR_CRFUNC_CR_FUNC, HashFunction::Init as u32);

    combo_hash.wo(utra::combohash::SFR_FR, 0xf); // clear completion flag
    combo_hash.wo(utra::combohash::SFR_AR, 0x5a); // start
    while combo_hash.rf(utra::combohash::SFR_FR_MFSM_DONE) == 0 {
        // wait for mem to copy
    }
    // clear the flag on exit
    combo_hash.rmwf(utra::combohash::SFR_FR_MFSM_DONE, 1);
}

pub fn sce_hash() {
    crate::println!("sce_hash(): init");
    setup();

    crate::println!("sce_hash(): init_hash");
    init_hash();

    crate::println!("sce_hash(): KAT starting");
    // SHA-512 Known Answer Test (KAT)
    const TEST_SHA512_DATA: &[u8] = b"0NlY  TH3   50Urce   c@n 5et You fr3e";
    const EXPECTED_HASH: [u8; 64] = hex_literal::hex!(
        "00000007a8d4c9f11f6ad8a8d71aa73a53c7ac392098f7a731b159d50586d7d08e5174218dbb2eaa7c9599165e6746e199410b9a86e74840052afa23e4976189"
    );
    let mut hasher = Sha512::new();
    hasher.update(TEST_SHA512_DATA);
    let digest: [u8; 64] = hasher.finalize().try_into().unwrap();
    if digest != EXPECTED_HASH {
        crate::println!("Sha2-512 KAT failed");
    } else {
        crate::println!("Sha2-512 KAT passed");
    }
}
