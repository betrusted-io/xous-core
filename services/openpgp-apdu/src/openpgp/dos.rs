//! OpenPGP card data objects (DOs) and BER-TLV helpers.

use super::fixtures::CardFixture;

pub fn encode_tlv(tag: u16, value: &[u8], out: &mut Vec<u8>) {
    if tag <= 0xFF {
        out.push(tag as u8);
    } else {
        out.push((tag >> 8) as u8);
        out.push(tag as u8);
    }
    let len = value.len();
    if len <= 0x7F {
        out.push(len as u8);
    } else if len <= 0xFF {
        out.push(0x81);
        out.push(len as u8);
    } else {
        out.push(0x82);
        out.push((len >> 8) as u8);
        out.push(len as u8);
    }
    out.extend_from_slice(value);
}

fn build_do_6e(fixture: &CardFixture) -> Vec<u8> {
    let mut inner = Vec::new();
    encode_tlv(0x004F, &fixture.aid, &mut inner);
    encode_tlv(0x00C0, fixture.extended_capabilities, &mut inner);
    encode_tlv(0x005F52, fixture.historical_bytes, &mut inner);
    inner
}

fn build_do_65(fixture: &CardFixture) -> Vec<u8> {
    let mut inner = Vec::new();
    encode_tlv(0x005B, fixture.name, &mut inner);
    encode_tlv(0x005F2D, fixture.language, &mut inner);
    encode_tlv(0x005F35, &[fixture.sex], &mut inner);
    let mut outer = Vec::new();
    encode_tlv(0x0065, &inner, &mut outer);
    outer
}

fn build_do_7a(fixture: &CardFixture) -> Vec<u8> {
    let ctr = fixture.signature_counter.to_be_bytes();
    let mut inner = Vec::new();
    encode_tlv(0x0093, &ctr[1..], &mut inner);
    let mut outer = Vec::new();
    encode_tlv(0x007A, &inner, &mut outer);
    outer
}

fn signature_counter_bytes(fixture: &CardFixture) -> Vec<u8> {
    fixture.signature_counter.to_be_bytes()[1..].to_vec()
}

/// Lookup DO payload for GET DATA tag `tag`.
pub fn get_do_data(tag: u16, fixture: &CardFixture) -> Option<Vec<u8>> {
    match tag {
        0x004F => Some(fixture.aid.to_vec()),
        0x005B => Some(fixture.name.to_vec()),
        0x005E => Some(fixture.login_data.to_vec()),
        0x005F2D => Some(fixture.language.to_vec()),
        0x005F35 => Some(vec![fixture.sex]),
        0x005F50 => Some(fixture.url.to_vec()),
        0x005F52 => Some(fixture.historical_bytes.to_vec()),
        0x0065 => Some(build_do_65(fixture)),
        0x006E => Some(build_do_6e(fixture)),
        0x007A => Some(build_do_7a(fixture)),
        0x0093 => Some(signature_counter_bytes(fixture)),
        0x00C0 => Some(fixture.extended_capabilities.to_vec()),
        0x00C4 => Some(fixture.pw_status.to_vec()),
        0x00C5 | 0x00CD => None,
        0x00C1 | 0x00C2 | 0x00C3 => None,
        0x00D6 => Some(vec![0x00, 0x00, 0x00]),
        0x007F74 => Some(vec![0x00]),
        0x0101 | 0x0102 | 0x0103 | 0x0104 => Some(Vec::new()),
        0x7F21 => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openpgp::fixtures::FIXTURE_V1_TEST;

    #[test]
    fn do_6e_contains_aid() {
        let data = get_do_data(0x006E, &FIXTURE_V1_TEST).unwrap();
        assert!(data.windows(5).any(|w| w == [0xD2, 0x76, 0x00, 0x01, 0x24]));
    }
}
