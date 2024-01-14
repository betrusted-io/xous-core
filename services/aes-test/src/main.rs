#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(feature = "low_level_tests")]
mod low_level_tests;

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Block};
use hex_literal::hex;

struct AesEcbTest<'a> {
    key: &'a [u8],
    plaintext: &'a [u8],
    ciphertext: &'a [u8],
}

impl<'a> AesEcbTest<'a> {
    pub fn new(key: &'a [u8], plaintext: &'a [u8], ciphertext: &'a [u8]) -> Self {
        Self { key, plaintext, ciphertext }
    }

    pub fn test(&self) -> Result<(), &'static str> {
        let mut output = Block::default();
        let aes = Aes128::new_from_slice(&self.key).unwrap();

        log::info!("Setting key");
        log::info!("Running encryption");
        aes.encrypt_block(&mut output);
        log::info!("Key:       {:x?}", self.key);
        log::info!("Plaintext: {:x?}", self.plaintext);
        log::info!("Reference: {:x?}", self.ciphertext);
        log::info!("Result:    {:x?}", output);
        if self.ciphertext.len() != output.len() {
            Err("encrypt error: ciphertext and output lengths do not match")?;
        }
        if self.ciphertext != output.as_slice() {
            Err("encrypt error: ciphertext and output values do not match")?;
        }

        log::info!("Running decryption");
        aes.decrypt_block(&mut output);
        log::info!("Plaintext: {:x?}", self.plaintext);
        log::info!("Result:    {:x?}", output);
        if self.plaintext.len() != output.len() {
            Err("decrypt error: plaintext and output lengths do not match")?;
        }
        if self.plaintext != output.as_slice() {
            Err("decrypt error: plaintext and output values do not match")?;
        }

        Ok(())
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::info!("Hello, world! This is the AES client, PID {}", xous::current_pid().unwrap().get());

    // let key: [u8; 32] = [
    //     0x70, 0x69, 0x19, 0xa0, 0x40, 0x61, 0x05, 0x17, 0xf7, 0xff, 0xf5, 0x27, 0x2b, 0x64, 0x04,
    //     0x67, 0xc5, 0x06, 0x7a, 0x4b, 0xba, 0x57, 0x78, 0xad, 0x6c, 0xdd, 0xcb, 0xf4, 0x73, 0x03,
    //     0x15, 0x64,
    // ];
    // let plaintext: [u8; 16] = [
    //     0x0b, 0x25, 0xf6, 0x7a, 0x11, 0xec, 0x9d, 0xf5, 0x73, 0x05, 0xfb, 0xe9, 0x48, 0x8a, 0xd6,
    //     0x1b,
    // ];
    // let reference: [u8; 16] = [
    //     0xc4, 0xb8, 0x9f, 0x45, 0x4e, 0xd8, 0x55, 0xa8, 0xa8, 0x63, 0x0b, 0xc8, 0x14, 0x87, 0x7e,
    //     0x94,
    // ];

    let tests = [
        // NIST ECB-AES128
        AesEcbTest::new(
            &hex!("2b7e151628aed2a6abf7158809cf4f3c"),
            &hex!("6bc1bee22e409f96e93d7e117393172a"),
            &hex!("3ad77bb40d7a3660a89ecaf32466ef97"),
        ),
        AesEcbTest::new(
            &hex!("2b7e151628aed2a6abf7158809cf4f3c"),
            &hex!("ae2d8a571e03ac9c9eb76fac45af8e51"),
            &hex!("f5d3d58503b9699de785895a96fdbaaf"),
        ),
        AesEcbTest::new(
            &hex!("2b7e151628aed2a6abf7158809cf4f3c"),
            &hex!("30c81c46a35ce411e5fbc1191a0a52ef"),
            &hex!("43b1cd7f598ece23881b00e3ed030688"),
        ),
        AesEcbTest::new(
            &hex!("2b7e151628aed2a6abf7158809cf4f3c"),
            &hex!("f69f2445df4f9b17ad2b417be66c3710"),
            &hex!("7b0c785e27e8ad3f8223207104725dd4"),
        ),
        // NIST ECB-AES192
        AesEcbTest::new(
            &hex!("8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b"),
            &hex!("6bc1bee22e409f96e93d7e117393172a"),
            &hex!("bd334f1d6e45f25ff712a214571fa5cc"),
        ),
        AesEcbTest::new(
            &hex!("8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b"),
            &hex!("ae2d8a571e03ac9c9eb76fac45af8e51"),
            &hex!("974104846d0ad3ad7734ecb3ecee4eef"),
        ),
        AesEcbTest::new(
            &hex!("8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b"),
            &hex!("30c81c46a35ce411e5fbc1191a0a52ef"),
            &hex!("ef7afd2270e2e60adce0ba2face6444e"),
        ),
        AesEcbTest::new(
            &hex!("8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b"),
            &hex!("f69f2445df4f9b17ad2b417be66c3710"),
            &hex!("9a4b41ba738d6c72fb16691603c18e0e"),
        ),
        // NIST ECB-AES256
        AesEcbTest::new(
            &hex!("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"),
            &hex!("6bc1bee22e409f96e93d7e117393172a"),
            &hex!("f3eed1bdb5d2a03c064b5a7e3db181f8"),
        ),
        AesEcbTest::new(
            &hex!("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"),
            &hex!("ae2d8a571e03ac9c9eb76fac45af8e51"),
            &hex!("591ccb10d410ed26dc5ba74a31362870"),
        ),
        AesEcbTest::new(
            &hex!("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"),
            &hex!("30c81c46a35ce411e5fbc1191a0a52ef"),
            &hex!("b6ed21b99ca6f4f9f153e7b1beafed1d"),
        ),
        AesEcbTest::new(
            &hex!("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"),
            &hex!("f69f2445df4f9b17ad2b417be66c3710"),
            &hex!("23304b7a39f9f3ff067d8d8f9e24ecc7"),
        ),
        // From the VexRiscv AES test
        AesEcbTest::new(
            &hex!("706919a040610517f7fff5272b640467c5067a4bba5778ad6cddcbf473031564"),
            &hex!("0b25f67a11ec9df57305fbe9488ad61b"),
            &hex!("c4b89f454ed855a8a8630bc814877e94"),
        ),
    ];

    let mut failures = 0;
    for test in &tests {
        if let Err(e) = test.test() {
            failures += 1;
            log::error!("Failed on test: {}", e);
        }
    }

    log::error!("{} tests were run and {} errors were encountered", tests.len(), failures);

    #[cfg(feature = "low_level_tests")]
    {
        log::info!("Running additional tests");

        low_level_tests::encrypt();
        low_level_tests::encrypt_last();
        low_level_tests::decrypt();
        low_level_tests::decrypt_last();
    }

    loop {
        xous::wait_event();
    }
}
