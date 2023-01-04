use packing::*;

mod fuzz_endian;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Packed)]
pub enum OpCode {
    A = 0x07,
    B = 0x11,
    C = 0xFF,
}

#[derive(Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeSense6Command {
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=0)] pub op_code: OpCode,
    #[packed(start_bit=3, end_bit=3, start_byte=1, end_byte=1)] pub disable_block_descriptors: bool,
    #[packed(start_bit=7, end_bit=6, start_byte=2, end_byte=2)] pub page_control: u8,
    #[packed(start_bit=5, end_bit=0, start_byte=2, end_byte=2)] pub page_code: u8,
    #[packed(start_bit=7, end_bit=0, start_byte=3, end_byte=3)] pub subpage_code: u8,
    #[packed(start_bit=7, end_bit=0, start_byte=4, end_byte=4)] pub allocation_length: u8,
    #[packed(start_bit=7, end_bit=0, start_byte=5, end_byte=5)] pub control: u8,
    #[packed(start_bit=7, end_bit=0, start_byte=6, end_byte=7)] pub sixteen: u16,
}

#[derive(Packed, PartialEq, Eq, Debug)]
#[packed(big_endian, lsb0)]
pub struct SomeBools {
    #[packed(start_bit=7, end_bit=7, start_byte=0, end_byte=0)] pub a: bool,
    #[packed(start_bit=5, end_bit=5, start_byte=0, end_byte=0)] pub b: bool,
    #[packed(start_bit=0, end_bit=0, start_byte=0, end_byte=0)] pub c: bool,
}

#[derive(Packed, PartialEq, Eq, Debug)]
#[packed(big_endian, lsb0)]
pub struct Nested {
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=0)] pub sb1: SomeBools,
    #[packed(start_bit=7, end_bit=0, start_byte=1, end_byte=1)] pub other1: u8,
    #[packed(start_bit=7, end_bit=0, start_byte=2, end_byte=2)] pub sb2: SomeBools,
    #[packed(start_bit=7, end_bit=0, start_byte=3, end_byte=4)] pub other2: u16,
    #[packed(start_bit=7, end_bit=0, start_byte=5, end_byte=5)] pub sb3: SomeBools,
}

#[derive(Packed, PartialEq, Debug)]
#[packed(big_endian, lsb0)]
pub struct Floats {
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=3)] pub a_float: f32,
    #[packed(start_bit=7, end_bit=0, start_byte=4, end_byte=11)] pub b_float: f64,
}

#[derive(Packed, PartialEq, Debug)]
#[packed(big_endian, lsb0)]
pub struct Integers {
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=0)] pub i8_: i8,
    #[packed(start_bit=7, end_bit=0, start_byte=1, end_byte=2)] pub i16_: i16,
    #[packed(start_bit=7, end_bit=0, start_byte=3, end_byte=6)] pub i32_: i32,
    #[packed(start_bit=7, end_bit=0, start_byte=7, end_byte=14)] pub i64_: i64,
    #[packed(start_bit=7, end_bit=0, start_byte=15, end_byte=30)] pub i128_: i128,

}



#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn qc_integers(i8_: i8, i16_: i16, i32_: i32, i64_: i64, i128_: i128) {
        let i = Integers { i8_, i16_, i32_, i64_, i128_ };

        let mut packed = [0; Integers::BYTES];
        i.pack(&mut packed).unwrap();

        let i2 = Integers::unpack(&packed).unwrap();
        assert_eq!(i, i2);

        let mut packed2 = [0; Integers::BYTES];
        i2.pack(&mut packed2).unwrap();
        assert_eq!(i, i2);
    }

    #[quickcheck]
    fn qc_floats(a_float: f32, b_float: f64) {
        let f = Floats { a_float, b_float };

        let mut packed = [0; Floats::BYTES];
        f.pack(&mut packed).unwrap();

        let f2 = Floats::unpack(&packed).unwrap();
        assert_eq!(f, f2);

        let mut packed2 = [0; Floats::BYTES];
        f2.pack(&mut packed2).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn test_nested_struct() {
        let n = Nested {
            sb1: SomeBools { a: true, b: false, c: true },
            other1: 5,
            sb2: SomeBools { a: true, b: true, c: true },
            other2: 260,
            sb3: SomeBools { a: false, b: true, c: false },
        };

        let mut packed = [0; Nested::BYTES];
        n.pack(&mut packed).unwrap();

        let n2 = Nested::unpack(&packed).unwrap();
        assert_eq!(n, n2);

        let mut packed2 = [0; Nested::BYTES];
        n2.pack(&mut packed2).unwrap();
        assert_eq!(n, n2);
    }

    #[test]
    fn test_mode_sense_6_unpack() {
        let op_code = OpCode::B;
        let disable_block_descriptors = true;
        let page_control = 3;
        let page_code = 4;
        let subpage_code = 222;
        let allocation_length = 1;
        let control = 41;
        let sixteen = u8::max_value() as u16 + 5;

        let sixteen_bytes = sixteen.to_be_bytes();

        let bytes = [
            op_code as u8,
            (disable_block_descriptors as u8) << 3,
            page_control << 6 | page_code,
            subpage_code,
            allocation_length,
            control,
            sixteen_bytes[0],
            sixteen_bytes[1],
        ];

        let cmd = ModeSense6Command::unpack(&bytes).unwrap();
        //assert_eq!(op_code, cmd.op_code);
        assert_eq!(disable_block_descriptors, cmd.disable_block_descriptors);
        assert_eq!(page_control, cmd.page_control);
        assert_eq!(page_code, cmd.page_code);
        assert_eq!(subpage_code, cmd.subpage_code);
        assert_eq!(allocation_length, cmd.allocation_length);
        assert_eq!(control, cmd.control);

        let mut packed = [0; ModeSense6Command::BYTES];
        cmd.pack(&mut packed).unwrap();
        assert_eq!(bytes, packed);
    }


    fn test_copy_field_bits_<En: Endian, S: Bit, E: Bit>(
        input_buffer: &[u8],
        expected_pack_result: &[u8],
        pack_buffer: &mut [u8],
        expected_unpack_result: &[u8],
        unpack_buffer: &mut [u8],
    ) -> Result<(), String> {
        assert!(expected_pack_result.len() == pack_buffer.len());
        assert!(expected_unpack_result.len() == unpack_buffer.len());

        let endian = if En::IS_LITTLE {
            "LittleEndian"
        } else {
            "BigEndian"
        };

        En::align_field_bits::<S, E>(input_buffer, pack_buffer);
        if let Err(e) = pretty_error(input_buffer, pack_buffer, expected_pack_result, S::USIZE, E::USIZE) {
            Err(format!("{}::align_field_bits{}", endian, e))?;
        }


        En::restore_field_bits::<S, E>(pack_buffer, unpack_buffer);

        if let Err(e) = pretty_error(pack_buffer, unpack_buffer, expected_unpack_result, S::USIZE, E::USIZE) {
            Err(format!("{}::restore_field_bits{}", endian, e))?;
        }

        Ok(())
    }

    #[test]
    fn test_copy_field_bits() {
        // These values are all hardcoded to hopefully eliminate duplicating simple errors
        // in the test code (<< 7-1 instead of << 8-1 for instance)
        // Some cases are from quickcheck runs and are added here to hopefully detect regressions


        // Single bit, should just mask and align it, BE and LE should be the same
        if let Err(e) = test_copy_field_bits_::<BigEndian, U5, U4>(
            &[  0b11111111 ],
            &[  0b00000011 ],
            &mut [0; 1],
            &[  0b0110000 ],
            &mut [0; 1],
        ) { panic!(e) };
        if let Err(e) = test_copy_field_bits_::<LittleEndian, U5, U4>(
            &[  0b11111111 ],
            &[  0b00000011 ],
            &mut [0; 1],
            &[  0b0110000 ],
            &mut [0; 1],
        ) { panic!(e) };

        // Derivative case, aligned & correct length
        if let Err(e) = test_copy_field_bits_::<BigEndian, U7, U0>(
            &[  0b11111111,     0b11111111 ],
            &[  0b11111111,     0b11111111 ],
            &mut [0; 2],
            &[  0b11111111,     0b11111111 ],
            &mut [0; 2],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U7, U0>(
            &[  0b11111111,     0b11111111 ],
            &[  0b11111111,     0b11111111 ],
            &mut [0; 2],
            &[  0b11111111,     0b11111111 ],
            &mut [0; 2],
        ) { panic!(e) }


        // Just head masking
        if let Err(e) = test_copy_field_bits_::<BigEndian, U5, U0>(
            &[  0b11111111,     0b11111111 ],
            &[  0b00111111,     0b11111111 ],
            &mut [0; 2],
            &[  0b00111111,     0b11111111 ],
            &mut [0; 2],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U5, U0>(
            &[  0b11111111,     0b11111111 ],
            &[  0b11111111,     0b00111111 ],
            &mut [0; 2],
            &[  0b00111111,     0b11111111 ],
            &mut [0; 2],
        ) { panic!(e) }


        // Just shifting
        if let Err(e) = test_copy_field_bits_::<BigEndian, U7, U2>(
            &[  0b11111111,     0b11111111 ],
            &[  0b00111111,     0b11111111 ],
            &mut [0; 2],
            &[  0b11111111,     0b11111100 ],
            &mut [0; 2],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U7, U2>(
            &[  0b11111111,     0b11111111 ],
            &[  0b11111111,     0b00111111 ],
            &mut [0; 2],
            &[  0b11111111,     0b11111100 ],
            &mut [0; 2],
        ) { panic!(e) }


        if let Err(e) = test_copy_field_bits_::<BigEndian, U7, U2>(
            &[  0b10000000,     0b11111111 ],
            &[  0b00100000,     0b00111111 ],
            &mut [0; 2],
            &[  0b10000000,     0b11111100 ],
            &mut [0; 2],
        ) { panic!(e) }


        if let Err(e) = test_copy_field_bits_::<LittleEndian, U7, U2>(
            &[  0b10000000,     0b11111111 ],
            &[  0b10000000,     0b00111111 ],
            &mut [0; 2],
            &[  0b10000000,     0b11111100 ],
            &mut [0; 2],
        ) { panic!(e) }


        // Masking and shifting
        if let Err(e) = test_copy_field_bits_::<BigEndian, U5, U2>(
            &[  0b11111111,     0b11111111 ],
            &[  0b00001111,     0b11111111 ],
            &mut [0; 2],
            &[  0b00111111,     0b11111100 ],
            &mut [0; 2],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<BigEndian, U5, U2>(
            &[  0b10111101,     0b11111000 ],
            &[  0b00001111,     0b01111110 ],
            &mut [0; 2],
            &[  0b00111101,     0b11111000 ],
            &mut [0; 2],
        ) { panic!(e) }


        // Shrinking
        if let Err(e) = test_copy_field_bits_::<BigEndian, U4, U5>(
            &[  0b11111111,     0b11111111 ],
            &[  0b11111111 ],
            &mut [0; 1],
            &[  0b00011111,     0b11100000 ],
            &mut [0; 2],
        ) { panic!(e) }


        // 1-byte -> 4-byte
        if let Err(e) = test_copy_field_bits_::<BigEndian, U7, U0>(
            &[  0b11111111 ],
            &[  0, 0, 0, 0b11111111 ],
            &mut [0; 4],
            &[  0b11111111 ],
            &mut [0; 1],
        ) { panic!(e) }


        // Bool behaviour
        if let Err(e) = test_copy_field_bits_::<BigEndian, U4, U4>(
            &[  0b00010000  ],
            &[  0b00000001  ],
            &mut [0; 1],
            &[  0b00010000  ],
            &mut [0; 1],
        ) { panic!(e) }


        // Some random cases from quicktest that found bugs previously
        if let Err(e) = test_copy_field_bits_::<LittleEndian, U2, U0>(
            &[  0b00000011  ],
            &[  0b00000011  ],
            &mut [0; 1],
            &[  0b00000011  ],
            &mut [0; 1],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<BigEndian, U4, U3>(
            &[  0b00011000  ],
            &[  0b00000011  ],
            &mut [0; 1],
            &[  0b00011000  ],
            &mut [0; 1],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U7, U2>(
            &[  0b00000100  ],
            &[  0b00000001  ],
            &mut [0; 1],
            &[  0b00000100  ],
            &mut [0; 1],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<BigEndian, U3, U2>(
            &[  0b00001100  ],
            &[  0b00000011  ],
            &mut [0; 1],
            &[  0b00001100  ],
            &mut [0; 1],
        ) { panic!(e) }

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U1, U6>(
            &[  0b00000010, 0b11000111  ],
            &[  0b00001110  ],
            &mut [0; 1],
            &[  0b00000010, 0b11000000  ],
            &mut [0; 2],
        ) { panic!(e) }

/*
        if let Err(e) = test_copy_field_bits_::<BigEndian, U3, U2>(
            &[  0b00001100  ],
            &[  0b00000011  ],
            &mut [0; 1],
            &[  0b00001100  ],
            &mut [0; 1],
        ) { panic!(e) }     */

        if let Err(e) = test_copy_field_bits_::<LittleEndian, U3, U1>(
            &[  0b00011100  ],
            &[  0b00000110  ],
            &mut [0; 1],
            &[  0b00001100  ],
            &mut [0; 1],
        ) { panic!(e) }

    }

    #[test]
    #[should_panic]
    fn test_copy_field_bits_too_big() {
        // 4-byte -> 1-byte
        if let Err(e) = test_copy_field_bits_::<BigEndian, U7, U0>(
            &[  0b11111111, 0b11111111, 0b11111111, 0b11111111 ],
            &[  0b11111111 ],
            &mut [0; 1],
            &[  0b00000000, 0b00000000, 0b00000000, 0b11111111 ],
            &mut [0; 4],
        ) { panic!(e) }
    }
}

pub fn pretty_error(input: &[u8], output: &[u8], expected: &[u8], s: usize, e: usize) -> Result<(), String> {
    let mut ok = true;

    if output.len() == expected.len() {
        for i in 0..expected.len() {
            ok &= expected[i] == output[i];
        }
    }

    if ok {
        return Ok(());
    }

    let mut ret = format!("<{}, {}> failed:\nInput      Output     Expected\n", s, e);
    for i in 0..input.len()
        .max(output.len())
        .max(expected.len())
    {
        if i < input.len() {
            ret += &format!("{:08b}   ", input[i]);
        } else {
            ret += "           ";
        }

        if i < output.len() {
            ret += &format!("{:08b}   ", output[i]);
        } else {
            ret += "           ";
        }

        if i < expected.len() {
            ret += &format!("{:08b}   ", expected[i]);
        } else {
            ret += "           ";
        }

        ret += "\n";
    }
    Err(ret)
}
