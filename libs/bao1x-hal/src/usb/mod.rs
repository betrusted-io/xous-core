#[cfg(all(not(feature = "std"), not(feature = "kernel")))]
extern crate alloc;
#[cfg(all(not(feature = "std"), not(feature = "kernel")))]
use alloc::string::{String, ToString};

#[cfg(not(feature = "std"))]
pub mod compat;
pub mod driver;
pub mod utra;

#[cfg(all(not(feature = "std"), not(feature = "kernel")))]
pub fn derive_usb_serial_number(
    owc: &crate::acram::OneWayCounter,
    slot_mgr: &crate::acram::SlotManager,
) -> String {
    use bao1x_api::{ExternalIdentifiers, UUID};

    match owc.get_decoded::<ExternalIdentifiers>().unwrap() {
        ExternalIdentifiers::Anonymous => "Anonymous".to_string(),
        ExternalIdentifiers::SerialNumber => {
            let uuid = slot_mgr.read(&UUID).unwrap();
            let mut ret = String::new();
            for (i, &src) in uuid.iter().enumerate() {
                let mut masked_src = src & 0b0001_1111;
                if (i == 0) && masked_src <= 10 {
                    masked_src += 11; // Avoid starting with a number or 'A'
                }
                // Translation table for charset "0123456789ABCDFGHJKLMNPQRSTVWXYZ" (32 symbols)
                // 0123456789 ABCD E FGH I JKLMN O PQRST U VWXYZ
                // 0000000000 1111 1 111 1 12222 2 22222 3 33333
                // 0123456789 0123 4 567 8 90123 4 56789 0 12345
                // 0000000000 1111   111   11122   22222   22233
                // 0123456789 0123   456   78901   23456   78901
                let c: char = char::from_u32(match masked_src {
                    x @ 0..=9 => ('0' as u8) + x,
                    x @ 10..=13 => ('A' as u8) + x - 10,
                    x @ 14..=16 => ('F' as u8) + x - 14,
                    x @ 17..=21 => ('J' as u8) + x - 17,
                    x @ 22..=26 => ('P' as u8) + x - 22,
                    x @ 27..=31 => ('V' as u8) + x - 27,
                    _ => '0' as u8,
                } as u32)
                .unwrap_or('.');
                ret.push(c);
                // long enough such that it's unlikely that two devices on one computer will have the same ID
                // short enough that it's likely to have a collision if used as a global UUID to fingerprint
                // devices
                if ret.len() == 6 {
                    break;
                }
            }
            ret
        }
    }
}
