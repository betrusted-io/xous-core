mod reference;

#[derive(Clone, Copy, Debug)]
enum AesId {
    AesId0 = 0,
    AesId1 = 1,
    AesId2 = 2,
    AesId3 = 3,
}

fn aes_enc_round(arg1: u32, arg2: u32, id: AesId) -> u32 {
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

fn aes_enc_round_last(arg1: u32, arg2: u32, id: AesId) -> u32 {
    extern "C" {
        fn vex_aes_enc_id_last_0(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_1(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_2(arg1: u32, arg2: u32) -> u32;
        fn vex_aes_enc_id_last_3(arg1: u32, arg2: u32) -> u32;
    }
    match id {
        AesId::AesId0 => unsafe { vex_aes_enc_id_last_0(arg1, arg2) },
        AesId::AesId1 => unsafe { vex_aes_enc_id_last_1(arg1, arg2) },
        AesId::AesId2 => unsafe { vex_aes_enc_id_last_2(arg1, arg2) },
        AesId::AesId3 => unsafe { vex_aes_enc_id_last_3(arg1, arg2) },
    }
}

fn aes_dec_round(arg1: u32, arg2: u32, id: AesId) -> u32 {
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

fn aes_dec_round_last(arg1: u32, arg2: u32, id: AesId) -> u32 {
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
            log::error!("Encrypt BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n", idx, id, dut, reference);
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
            // return;
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
            log::error!("Decrypt BUG at index {}, {:?}: dut {:08x} != {:08x} ref\n", idx, id, dut, reference);
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

pub fn encrypt() {
    log::info!("Testing Encrypt AesId 0");
    encrypt_test(AesId::AesId0);
    log::info!("Testing Encrypt AesId 1");
    encrypt_test(AesId::AesId1);
    log::info!("Testing Encrypt AesId 2");
    encrypt_test(AesId::AesId2);
    log::info!("Testing Encrypt AesId 3");
    encrypt_test(AesId::AesId3);
}

pub fn encrypt_last() {
    log::info!("Testing Encrypt Last AesId 0");
    encrypt_test_last(AesId::AesId0);
    log::info!("Testing Encrypt Last AesId 1");
    encrypt_test_last(AesId::AesId1);
    log::info!("Testing Encrypt Last AesId 2");
    encrypt_test_last(AesId::AesId2);
    log::info!("Testing Encrypt Last AesId 3");
    encrypt_test_last(AesId::AesId3);
}

pub fn decrypt() {
    log::info!("Testing Decrypt AesId 0");
    decrypt_test(AesId::AesId0);
    log::info!("Testing Decrypt AesId 1");
    decrypt_test(AesId::AesId1);
    log::info!("Testing Decrypt AesId 2");
    decrypt_test(AesId::AesId2);
    log::info!("Testing Decrypt AesId 3");
    decrypt_test(AesId::AesId3);
}

pub fn decrypt_last() {
    log::info!("Testing Decrypt Last AesId 0");
    decrypt_test_last(AesId::AesId0);
    log::info!("Testing Decrypt Last AesId 1");
    decrypt_test_last(AesId::AesId1);
    log::info!("Testing Decrypt Last AesId 2");
    decrypt_test_last(AesId::AesId2);
    log::info!("Testing Decrypt Last AesId 3");
    decrypt_test_last(AesId::AesId3);
}
