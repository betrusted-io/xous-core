// this code is based on https://github.com/Keats/rust-bcrypt
// the original crate is not used directly because the base64 wrappers cause the crate
// to be incompatible with a no-std environment. We are storing binary data directly
// in our use case.

// Cost constants
#[allow(dead_code)]
const MIN_COST: u32 = 4;
#[allow(dead_code)]
const MAX_COST: u32 = 31;

use blowfish::Blowfish;

fn setup(cost: u32, salt: &[u8], key: &[u8]) -> Blowfish {
    assert!(cost < 32);
    let mut state = Blowfish::bc_init_state();

    state.salted_expand_key(salt, key);
    for _ in 0..1u32 << cost {
        state.bc_expand_key(key);
        state.bc_expand_key(salt);
    }

    state
}

pub fn bcrypt(cost: u32, salt: &[u8], pw: &str, output: &mut [u8]) {
    assert!(salt.len() == 16);
    assert!(output.len() == 24);

    let pw_len = if pw.len() > 72 {
        log::warn!("password of length {} is truncated to 72 bytes [reason: bcrypt limitation]", pw.len());
        72
    } else {
        pw.len() + 1
    };
    let mut plaintext_copy: [u8; 73] = [0; 73];
    for (src, dst) in pw.bytes().zip(plaintext_copy.iter_mut()) {
        *dst = src;
    }
    plaintext_copy[72] = 0; // always null terminate

    // this function takes the plaintext key and uses it to prime a ~4k region of stack with an s-box
    // that's used for the round function. The upstream Rust crypto crate does not wipe the sbox after use.
    // however, it seems non-trivial to reverse the original password from the s-boxes.
    let state = setup(cost, salt, &plaintext_copy[..pw_len]);

    // erase the plaintext copy as soon as we're done with it
    // an unsafe method is used because the compiler will correctly reason that plaintext_copy goes out of
    // scope and these writes are never read, and therefore they may be optimized out.
    let pt_ptr = plaintext_copy.as_mut_ptr();
    for i in 0..plaintext_copy.len() {
        unsafe {
            pt_ptr.add(i).write_volatile(core::mem::zeroed());
        }
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    // OrpheanBeholderScryDoubt
    #[allow(clippy::unreadable_literal)]
    let mut ctext = [0x4f727068, 0x65616e42, 0x65686f6c, 0x64657253, 0x63727944, 0x6f756274];
    for i in 0..3 {
        let i: usize = i * 2;
        for _ in 0..64 {
            let (l, r) = state.bc_encrypt(ctext[i], ctext[i + 1]);
            ctext[i] = l;
            ctext[i + 1] = r;
        }

        let buf = ctext[i].to_be_bytes();
        output[i * 4..][..4].copy_from_slice(&buf);
        let buf = ctext[i + 1].to_be_bytes();
        output[(i + 1) * 4..][..4].copy_from_slice(&buf);
    }
}
