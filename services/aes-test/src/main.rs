#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod reference;
// mod aes;

#[derive(Clone, Copy, Debug)]
pub enum AesId {
    AesId0 = 0,
    AesId1 = 1,
    AesId2 = 2,
    AesId3 = 3,
}

pub fn aes_enc_round(arg1: u32, arg2: u32, id: AesId) -> u32 {
    extern "C" {
        fn vex_aes_enc_id_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesId::AesId0 => unsafe { vex_aes_enc_id_0(arg1, arg2) },
        AesId::AesId1 => unsafe { vex_aes_enc_id_1(arg1, arg2) },
        AesId::AesId2 => unsafe { vex_aes_enc_id_2(arg1, arg2) },
        AesId::AesId3 => unsafe { vex_aes_enc_id_3(arg1, arg2) },
    }
}

pub fn aes_enc_round_last(arg1: u32, arg2: u32, id: AesId) -> u32 {
    extern "C" {
        fn vex_aes_dec_id_last_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesId::AesId0 => unsafe { vex_aes_dec_id_last_0(arg1, arg2) },
        AesId::AesId1 => unsafe { vex_aes_enc_id_last_1(arg1, arg2) },
        AesId::AesId2 => unsafe { vex_aes_enc_id_last_2(arg1, arg2) },
        AesId::AesId3 => unsafe { vex_aes_enc_id_last_3(arg1, arg2) },
    }
}

pub fn aes_dec_round(arg1: u32, arg2: u32, id: AesId) -> u32 {
    extern "C" {
        fn vex_aes_dec_id_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesId::AesId0 => unsafe { vex_aes_dec_id_0(arg1, arg2) },
        AesId::AesId1 => unsafe { vex_aes_dec_id_1(arg1, arg2) },
        AesId::AesId2 => unsafe { vex_aes_dec_id_2(arg1, arg2) },
        AesId::AesId3 => unsafe { vex_aes_dec_id_3(arg1, arg2) },
    }
}

pub fn aes_dec_round_last(arg1: u32, arg2: u32, id: AesId) -> u32 {
    extern "C" {
        fn vex_aes_dec_id_last_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_last_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_last_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_dec_id_last_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesId::AesId0 => unsafe { vex_aes_dec_id_last_0(arg1, arg2) },
        AesId::AesId1 => unsafe { vex_aes_dec_id_last_1(arg1, arg2) },
        AesId::AesId2 => unsafe { vex_aes_dec_id_last_2(arg1, arg2) },
        AesId::AesId3 => unsafe { vex_aes_dec_id_last_3(arg1, arg2) },
    }
}

fn encrypt_test(id: AesId) {
    log::info!("Testing Encrypt, ID {:?}", id);
    let mut dut: u32 = 0;
    let mut reference: u32 = 0;
    for (idx, byte) in reference::ENCRYPT_REF[id as usize].iter().enumerate() {
        let tmp = idx << ((id as usize) * 8);
        dut = aes_enc_round(dut, tmp as u32, id);
        reference ^= byte;
        if dut != reference {
            log::error!(
                "Encrypt BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n",
                idx,
                id,
                dut,
                reference
            );
            return;
        }
    }
    log::info!("Pass");
}

fn encrypt_test_last(id: AesId) {
    log::info!("Testing Encrypt Last, ID {:?}", id);
    let mut dut: u32 = 0;
    let mut reference: u32 = 0;
    for (idx, byte) in reference::ENCRYPT_LAST_REF[id as usize].iter().enumerate() {
        let tmp = idx << ((id as usize) * 8);
        dut = aes_enc_round_last(dut, tmp as u32, id);
        reference ^= byte;
        if dut != reference {
            log::error!(
                "Encrypt Last BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n",
                idx,
                id,
                dut,
                reference
            );
            return;
        }
    }
    log::info!("Pass");
}

fn decrypt_test(id: AesId) {
    log::info!("Testing Decrypt, ID {:?}", id);
    let mut dut: u32 = 0;
    let mut reference: u32 = 0;
    for (idx, byte) in reference::DECRYPT_REF[id as usize].iter().enumerate() {
        let tmp = idx << ((id as usize) * 8);
        dut = aes_dec_round(dut, tmp as u32, id);
        reference ^= byte;
        if dut != reference {
            log::error!(
                "Decrypt BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n",
                idx,
                id,
                dut,
                reference
            );
            return;
        }
    }
    log::info!("Pass");
}

fn decrypt_test_last(id: AesId) {
    log::info!("Testing Decrypt Last, ID {:?}", id);
    let mut dut: u32 = 0;
    let mut reference: u32 = 0;
    for (idx, byte) in reference::DECRYPT_LAST_REF[id as usize].iter().enumerate() {
        let tmp = idx << ((id as usize) * 8);
        dut = aes_dec_round_last(dut, tmp as u32, id);
        reference ^= byte;
        if dut != reference {
            log::error!(
                "Decrypt Last BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n",
                idx,
                id,
                dut,
                reference
            );
            return;
        }
    }
    log::info!("Pass");
}

#[xous::xous_main]
fn aes_test_main() -> ! {
    log_server::init_wait().unwrap();
    log::info!(
        "Hello, world! This is the AES client, PID {}",
        xous::current_pid().unwrap().get()
    );

    log::info!("Testing Encrypt AesId 0");
    encrypt_test(AesId::AesId0);
    log::info!("Testing Encrypt AesId 1");
    encrypt_test(AesId::AesId1);
    log::info!("Testing Encrypt AesId 2");
    encrypt_test(AesId::AesId2);
    log::info!("Testing Encrypt AesId 3");
    encrypt_test(AesId::AesId3);

    log::info!("Testing Encrypt Last AesId 0");
    encrypt_test_last(AesId::AesId0);
    log::info!("Testing Encrypt Last AesId 1");
    encrypt_test_last(AesId::AesId1);
    log::info!("Testing Encrypt Last AesId 2");
    encrypt_test_last(AesId::AesId2);
    log::info!("Testing Encrypt Last AesId 3");
    encrypt_test_last(AesId::AesId3);

    log::info!("Testing Decrypt AesId 0");
    decrypt_test(AesId::AesId0);
    log::info!("Testing Decrypt AesId 1");
    decrypt_test(AesId::AesId1);
    log::info!("Testing Decrypt AesId 2");
    decrypt_test(AesId::AesId2);
    log::info!("Testing Decrypt AesId 3");
    decrypt_test(AesId::AesId3);

    log::info!("Testing Decrypt Last AesId 0");
    decrypt_test_last(AesId::AesId0);
    log::info!("Testing Decrypt Last AesId 1");
    decrypt_test_last(AesId::AesId1);
    log::info!("Testing Decrypt Last AesId 2");
    decrypt_test_last(AesId::AesId2);
    log::info!("Testing Decrypt Last AesId 3");
    decrypt_test_last(AesId::AesId3);

    let key: [u8; 32] = [
        0x70, 0x69, 0x19, 0xa0, 0x40, 0x61, 0x05, 0x17, 0xf7, 0xff, 0xf5, 0x27, 0x2b, 0x64, 0x04,
        0x67, 0xc5, 0x06, 0x7a, 0x4b, 0xba, 0x57, 0x78, 0xad, 0x6c, 0xdd, 0xcb, 0xf4, 0x73, 0x03,
        0x15, 0x64,
    ];
    let plaintext: [u8; 16] = [
        0x0b, 0x25, 0xf6, 0x7a, 0x11, 0xec, 0x9d, 0xf5, 0x73, 0x05, 0xfb, 0xe9, 0x48, 0x8a, 0xd6,
        0x1b,
    ];
    let reference: [u8; 16] = [
        0xc4, 0xb8, 0x9f, 0x45, 0x4e, 0xd8, 0x55, 0xa8, 0xa8, 0x63, 0x0b, 0xc8, 0x14, 0x87, 0x7e,
        0x94,
    ];

    loop {
        xous::wait_event();
    }
}
