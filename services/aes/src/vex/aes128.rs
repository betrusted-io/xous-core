use crate::vex::*;

/// AES-128 round keys
pub(crate) type VexKeys128 = [u32; 60];

pub fn aes128_enc_key_schedule(user_key: &[u8]) -> VexKeys128 {
    set_encrypt_key_inner_128(user_key, true)
}

fn set_encrypt_key_inner_128(user_key: &[u8], swap_final: bool) -> VexKeys128 {
    let mut rk: VexKeys128 = [0; 60];

    rk[0] = get_u32_be(user_key, 0);
    rk[1] = get_u32_be(user_key, 4);
    rk[2] = get_u32_be(user_key, 8);
    rk[3] = get_u32_be(user_key, 12);
    let mut rk_offset = 0;
    for rcon in &RCON {
        let temp = rk[3 + rk_offset] as usize;
        rk[4 + rk_offset] = rk[0 + rk_offset]
            ^ (TE2[(temp >> 16) & 0xff] & 0xff000000)
            ^ (TE3[(temp >> 8) & 0xff] & 0x00ff0000)
            ^ (TE0[(temp) & 0xff] & 0x0000ff00)
            ^ (TE1[temp >> 24] & 0x000000ff)
            ^ rcon;
        rk[5 + rk_offset] = rk[1 + rk_offset] ^ rk[4 + rk_offset];
        rk[6 + rk_offset] = rk[2 + rk_offset] ^ rk[5 + rk_offset];
        rk[7 + rk_offset] = rk[3 + rk_offset] ^ rk[6 + rk_offset];
        rk_offset += 4;
    }
    if swap_final {
        for value in &mut rk {
            *value = value.swap_bytes();
        }
    }
    rk
}

pub fn aes128_dec_key_schedule(user_key: &[u8]) -> VexKeys128 {
    let mut rk = set_encrypt_key_inner_128(user_key, false);

    let rounds = 10;

    /* invert the order of the round keys: */
    let mut i = 0;
    let mut j = 4 * rounds;
    while i < j {
        let temp = rk[i];
        rk[i] = rk[j];
        rk[j] = temp;

        let temp = rk[i + 1];
        rk[i + 1] = rk[j + 1];
        rk[j + 1] = temp;

        let temp = rk[i + 2];
        rk[i + 2] = rk[j + 2];
        rk[j + 2] = temp;

        let temp = rk[i + 3];
        rk[i + 3] = rk[j + 3];
        rk[j + 3] = temp;

        i += 4;
        j -= 4;
    }

    /* apply the inverse MixColumn transform to all round keys but the first and the last: */
    let mut rk_offset = 4;
    for _ in 1..rounds {
        rk[0 + rk_offset] = TD0[TE1[rk[0 + rk_offset] as usize >> 24] as usize & 0xff]
            ^ TD1[TE1[(rk[0 + rk_offset] as usize >> 16) & 0xff] as usize & 0xff]
            ^ TD2[TE1[(rk[0 + rk_offset] as usize >> 8) & 0xff] as usize & 0xff]
            ^ TD3[TE1[(rk[0 + rk_offset] as usize) & 0xff] as usize & 0xff];
        rk[1 + rk_offset] = TD0[TE1[rk[1 + rk_offset] as usize >> 24] as usize & 0xff]
            ^ TD1[TE1[(rk[1 + rk_offset] as usize >> 16) & 0xff] as usize & 0xff]
            ^ TD2[TE1[(rk[1 + rk_offset] as usize >> 8) & 0xff] as usize & 0xff]
            ^ TD3[TE1[(rk[1 + rk_offset] as usize) & 0xff] as usize & 0xff];
        rk[2 + rk_offset] = TD0[TE1[rk[2 + rk_offset] as usize >> 24] as usize & 0xff]
            ^ TD1[TE1[(rk[2 + rk_offset] as usize >> 16) & 0xff] as usize & 0xff]
            ^ TD2[TE1[(rk[2 + rk_offset] as usize >> 8) & 0xff] as usize & 0xff]
            ^ TD3[TE1[(rk[2 + rk_offset] as usize) & 0xff] as usize & 0xff];
        rk[3 + rk_offset] = TD0[TE1[rk[3 + rk_offset] as usize >> 24] as usize & 0xff]
            ^ TD1[TE1[(rk[3 + rk_offset] as usize >> 16) & 0xff] as usize & 0xff]
            ^ TD2[TE1[(rk[3 + rk_offset] as usize >> 8) & 0xff] as usize & 0xff]
            ^ TD3[TE1[(rk[3 + rk_offset] as usize) & 0xff] as usize & 0xff];
        rk_offset += 4;
    }

    for value in &mut rk {
        *value = value.swap_bytes();
    }
    rk
}
