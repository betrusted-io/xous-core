pub mod efuse_ecc {
    /// given an unprotected 24-bit data record, return
    /// a number which is the data + its 6-bit ECC code
    pub fn add_ecc(data: u32) -> u32 {
        assert!(data & 0xFF00_0000 == 0); // if the top 8 bits are filled in, that's an error
        const GENERATOR: [u32; 6] = [16_515_312, 14_911_249, 10_180_898, 5_696_068, 3_011_720, 16_777_215];

        let mut code: u32 = 0;

        for (i, gen) in GENERATOR.iter().enumerate() {
            let mut parity: u32 = 0;
            for bit in 0..24 {
                parity ^= ((gen & data) >> bit) & 0x1;
            }
            code ^= parity << i;
        }

        if (code & 0x20) != 0 {
            code = (!code & 0x1F) | 0x20;
        }

        let secded = ((((code >> 5) ^ (code >> 4) ^ (code >> 3) ^ (code >> 2) ^ (code >> 1) ^ code) & 0x1)
            << 5)
            | code;

        data | secded << 24
    }
}

// run with `cargo test --target x86_64-unknown-linux-gnu`
#[cfg(test)]
mod tests {
    use crate::efuse_ecc::*;

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn vectors() {
        const V: [(u32, u32); 7] = [
            (0x00_FFFFFD, 0x25_FFFFFD),
            (0x00_00A003, 0x24_00A003),
            (0x00_00A00A, 0x36_00A00A),
            (0x00_00F00A, 0x1E_00F00A),
            (0x00_00F00F, 0x14_00F00F),
            (0x00_00B00F, 0x37_00B00F),
            (0x00_C5B000, 0x2A_C5B000),
        ];

        for i in &V {
            assert_eq!(i.1, add_ecc(i.0));
        }
    }

    #[test]
    fn gen_test() {
        assert_eq!(0x2708_63C1, add_ecc(0x8_63C1));
        assert_eq!(0x2C02_A541, add_ecc(0x2_A541));
        assert_eq!(0x00CC_ABCD, add_ecc(0xCC_ABCD));
        assert_eq!(0x03C6_DEF0, add_ecc(0xC6_DEF0));
        assert_eq!(0x3944_EEEE, add_ecc(0x44_EEEE));
    }
}
