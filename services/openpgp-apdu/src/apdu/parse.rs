//! ISO 7816-4 short and extended command/response APDU parsing.

use super::status::StatusWord;

#[derive(Debug, Eq, PartialEq)]
pub struct CommandApdu {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: Vec<u8>,
    pub le: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApduError {
    Empty,
    TooShort,
    InconsistentLengths,
}

impl CommandApdu {
    pub fn parse(raw: &[u8]) -> Result<Self, ApduError> {
        if raw.is_empty() {
            return Err(ApduError::Empty);
        }
        if raw.len() < 4 {
            return Err(ApduError::TooShort);
        }
        let cla = raw[0];
        let ins = raw[1];
        let p1 = raw[2];
        let p2 = raw[3];

        if raw.len() == 4 {
            return Ok(Self {
                cla,
                ins,
                p1,
                p2,
                data: Vec::new(),
                le: None,
            });
        }

        if raw.len() == 5 {
            return Ok(Self {
                cla,
                ins,
                p1,
                p2,
                data: Vec::new(),
                le: Some(le_short(raw[4])),
            });
        }

        let l0 = raw[4];
        if l0 == 0 && raw.len() > 7 {
            let lc = u16::from_be_bytes([raw[5], raw[6]]) as usize;
            let data_end = 7usize.saturating_add(lc);
            if raw.len() < data_end {
                return Err(ApduError::InconsistentLengths);
            }
            let data = raw[7..data_end].to_vec();
            let le = parse_le(raw, data_end)?;
            return Ok(Self {
                cla,
                ins,
                p1,
                p2,
                data,
                le,
            });
        }

        let lc = l0 as usize;
        let data_start = 5usize;
        let data_end = data_start.saturating_add(lc);
        if raw.len() < data_end {
            return Err(ApduError::InconsistentLengths);
        }
        let data = raw[data_start..data_end].to_vec();
        let le = parse_le(raw, data_end)?;
        Ok(Self {
            cla,
            ins,
            p1,
            p2,
            data,
            le,
        })
    }
}

fn parse_le(raw: &[u8], data_end: usize) -> Result<Option<u16>, ApduError> {
    if raw.len() <= data_end {
        return Ok(None);
    }
    if raw.len() == data_end + 1 {
        return Ok(Some(le_short(raw[data_end])));
    }
    if raw.len() == data_end + 2 {
        if raw[data_end] == 0 {
            Ok(Some(u16::from_be_bytes([0, raw[data_end + 1]])))
        } else {
            Ok(Some(u16::from_be_bytes([raw[data_end], raw[data_end + 1]])))
        }
    } else if raw.len() == data_end + 3 && raw[data_end] == 0 {
        Ok(Some(u16::from_be_bytes([raw[data_end + 1], raw[data_end + 2]])))
    } else {
        Err(ApduError::InconsistentLengths)
    }
}

fn le_short(b: u8) -> u16 {
    if b == 0 {
        256
    } else {
        u16::from(b)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ResponseApdu {
    pub data: Vec<u8>,
    pub sw1: u8,
    pub sw2: u8,
}

impl ResponseApdu {
    pub fn ok(data: Vec<u8>) -> Self {
        Self {
            data,
            sw1: StatusWord::Success.sw1(),
            sw2: StatusWord::Success.sw2(),
        }
    }

    pub fn ok_empty() -> Self {
        Self::ok(Vec::new())
    }

    pub fn error(sw: StatusWord) -> Self {
        Self {
            data: Vec::new(),
            sw1: sw.sw1(),
            sw2: sw.sw2(),
        }
    }

    pub fn with_status(data: Vec<u8>, sw: StatusWord) -> Self {
        Self {
            data,
            sw1: sw.sw1(),
            sw2: sw.sw2(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.data.clone();
        out.push(self.sw1);
        out.push(self.sw2);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_select_apdu() {
        let raw = [0x00u8, 0xA4, 0x04, 0x00, 0x06, 0xD2, 0x76, 0x00, 0x01, 0x24, 0x01];
        let c = CommandApdu::parse(&raw).unwrap();
        assert_eq!(c.ins, 0xA4);
        assert_eq!(c.data, [0xD2, 0x76, 0x00, 0x01, 0x24, 0x01]);
        assert_eq!(c.le, None);
    }

    #[test]
    fn parse_get_data_apdu() {
        let raw = [0x00u8, 0xCA, 0x00, 0x4F, 0x00];
        let c = CommandApdu::parse(&raw).unwrap();
        assert_eq!(c.le, Some(256));
    }
}
