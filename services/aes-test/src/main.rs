#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod aes;

#[cfg(feature = "low_level_tests")]
mod low_level_tests;

#[xous::xous_main]
fn aes_test_main() -> ! {
    log_server::init_wait().unwrap();
    log::info!(
        "Hello, world! This is the AES client, PID {}",
        xous::current_pid().unwrap().get()
    );

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

    // let key = [
    //     0x2B, 0x7E, 0x15, 0x16, 0x28, 0xAE, 0xD2, 0xA6, 0xAB, 0xF7, 0x15, 0x88, 0x09, 0xCF, 0x4F,
    //     0x3C,
    // ];
    // let plaintext = [
    //     0x6B, 0xC1, 0xBE, 0xE2, 0x2E, 0x40, 0x9F, 0x96, 0xE9, 0x3D, 0x7E, 0x11, 0x73, 0x93, 0x17,
    //     0x2A,
    // ];
    // let reference = [
    //     0x3A, 0xD7, 0x7B, 0xB4, 0x0D, 0x7A, 0x36, 0x60, 0xA8, 0x9E, 0xCA, 0xF3, 0x24, 0x66, 0xEF,
    //     0x97,
    // ];

    let mut output = [0u8; 16];
    let mut aes_key = Default::default();
    log::info!("Setting key");
    aes::set_encrypt_key(&key, &mut aes_key).unwrap();
    log::info!("Running encryption");
    aes::vexriscv_aes_encrypt(&plaintext, &mut output, &aes_key);
    log::info!("Plaintext: {:?}", plaintext);
    log::info!("Key:       {:?}", key);
    log::info!("Reference: {:?}", reference);
    log::info!("Result:    {:?}", output);

    aes::set_decrypt_key(&key, &mut aes_key).unwrap();
    log::info!("Running decryption");
    aes::vexriscv_aes_decrypt(&reference, &mut output, &aes_key);
    log::info!("Plaintext: {:?}", plaintext);
    log::info!("Result:    {:?}", output);

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
