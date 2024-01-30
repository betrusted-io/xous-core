/// Eventually more language support can be added here:
///
/// In order to integrate this well, we need to re-do the language build
/// system to be based off of af #cfg feature, so that we can pick up
/// the feature in this crate and select the right word list.
///
/// We don't compile all the word lists in because code size is precious.
///
/// Each language should simply create its table assigning to be symbol
/// `const BIP39_TABLE: [&'static str; 2048]`. This allows the rest of
/// the code to refer to the table without change, all we do is swap out
/// which language module is included in the two lines below.
pub mod en;
use digest::Digest;
pub use en::*;
use sha2::*;

#[derive(Debug, Eq, PartialEq)]
pub enum Bip39Error {
    InvalidLength,
    InvalidChecksum,
    InvalidWordAt(usize),
}

/// This routine takes an array of bytes and attempts to return an array of Bip39
/// words. If the bytes do not conform to a valid length, we return `Bip39Error::InvalidLength`.
/// A `Vec::<String>` is returned in case the caller wants to do stupid formatting tricks
/// on the words (saving them effort of parsing a single concatenated String).
pub(crate) fn bytes_to_bip39(bytes: &Vec<u8>) -> Result<Vec<String>, Bip39Error> {
    let mut result = Vec::<String>::new();
    match bytes.len() {
        16 | 20 | 24 | 28 | 32 => (),
        _ => return Err(Bip39Error::InvalidLength),
    }
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let checksum_bits = bytes.len() / 4;
    let checksum = digest.as_slice()[0] >> (8 - checksum_bits);

    let mut bits_in_bucket = 0;
    let mut bucket = 0u32;
    for &b in bytes {
        bucket <<= 8;
        bucket |= b as u32;
        bits_in_bucket += 8;
        if bits_in_bucket >= 11 {
            let codeword = bucket >> (bits_in_bucket - 11);
            bucket &= !((0b111_1111_1111u32) << (bits_in_bucket - 11));
            bits_in_bucket -= 11;
            result.push(BIP39_TABLE[codeword as usize].to_string());
        }
    }
    assert!(bits_in_bucket + checksum_bits == 11);
    bucket <<= checksum_bits;
    bucket |= checksum as u32;
    assert!(bucket < 2048);
    result.push(BIP39_TABLE[bucket as usize].to_string());
    Ok(result)
}

/// The caller must provide a list of words parsed into individual Bip39 words.
/// The words are case-insensitive. However, if any word is invalid, the routine
/// will return `InvalidWordAt(index of invalid word)` at the first invalid word
/// detected (it may not be the only invalid word).
pub(crate) fn bip39_to_bytes(bip39: &Vec<String>) -> Result<Vec<u8>, Bip39Error> {
    // this implementation favors small runtime memory allocation over performance
    // sifting through a list of 2048 words is reasonably fast, even if doing up to 24 times;
    // this is especially in comparison to the screen redraw times. We could also create
    // a HashSet or something or a tree to do the search faster but in this system, we fight
    // for even 4kiB of RAM savings at times.
    // The inefficiency is especially small in comparison to the ridiculous SHA256 computation
    // that has to happen to checksum the result.

    match bip39.len() {
        12 | 15 | 18 | 21 | 24 => (),
        _ => return Err(Bip39Error::InvalidLength),
    }

    let mut indices = Vec::<u32>::new();
    for (index, bip) in bip39.iter().enumerate() {
        if let Some(i) = BIP39_TABLE.iter().position(|&x| x == bip) {
            indices.push(i as u32);
        } else {
            return Err(Bip39Error::InvalidWordAt(index));
        }
    }

    // collate into u8 vec
    let mut data = Vec::<u8>::new();
    let mut bucket = 0u32;
    let mut bits_in_bucket = 0;
    for index in indices {
        // add bits to bucket
        bucket = (bucket << 11) | index;
        bits_in_bucket += 11;

        while bits_in_bucket >= 8 {
            // extract the top 8 bits from the bucket, put it into the result vector
            data.push((bucket >> (bits_in_bucket - 8)) as u8);
            // mask off the "used up" bits
            bucket &= !(0b1111_1111 << bits_in_bucket - 8);

            // subtract the used bits out of the bucket
            bits_in_bucket -= 8;
        }
    }
    // the bucket should now just contain the checksum
    let entered_checksum = if bits_in_bucket == 0 {
        // edge case of exactly enough checksum bits to fill a byte (happens in 256-bit case)
        data.pop().unwrap()
    } else {
        bucket as u8
    };

    let mut hasher = sha2::Sha256::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    let checksum_bits = data.len() / 4;
    let checksum = digest.as_slice()[0] >> (8 - checksum_bits);
    if checksum == entered_checksum {
        Ok(data)
    } else {
        log::warn!("checksum didn't match: {:x} vs {:x}", checksum, entered_checksum);
        Err(Bip39Error::InvalidChecksum)
    }
}

pub(crate) const BIP39_SUGGEST_LIMIT: usize = 5;
/// This turns a string into a list of suggestions. If the String is empty, the
/// suggestion list is empty. The suggestion list is limited to BIP39_SUGGEST_LIMIT hints.
pub(crate) fn suggest_bip39(start: &str) -> Vec<String> {
    let mut ret = Vec::<String>::new();
    // first see if any prefixes match; stop when we find enough
    for bip in BIP39_TABLE {
        if bip.starts_with(start) {
            ret.push(bip.to_string());
            if ret.len() >= BIP39_SUGGEST_LIMIT {
                break;
            }
        }
    }
    if ret.len() > 0 {
        return ret;
    }
    // no prefixes match, suggest substrings
    for bip in BIP39_TABLE {
        if bip.contains(start) {
            ret.push(bip.to_string());
            if ret.len() >= BIP39_SUGGEST_LIMIT {
                break;
            }
        }
    }
    ret
}

/// This routine returns `true` if the given word is a valid BIP39 word.
#[allow(dead_code)]
pub(crate) fn is_valid_bip39(word: &str) -> bool {
    let lword = word.to_ascii_lowercase();
    for w in BIP39_TABLE {
        if lword == w {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// PAGE_SIZE is required to be a power of two. 0x1000 -> 0x1000 - 1 = 0xFFF, which forms the bitmasks.
    fn test_11_to_8() {
        let indices = [
            0b00000110001,
            0b10110011110,
            0b01110010100,
            0b00110110010,
            0b10001011010,
            0b11100111111,
            0b01101010011,
            0b10000011000,
            0b01101011001,
            0b10110011111,
            0b10001001110,
            0b00111100110,
        ];
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;

        // 00000110001 10110011110 01110010100 00110110010 10001011010 11100111111 01101010011 10000011000
        // 01101011001 10110011111 10001001110 00111100110 0000_0110|001/1_0110|0111_10/
        // 01|1100_1010_0/001_10110010 10001011010 11100111111 01101010011 10000011000 01101011001 10110011111
        // 10001001110 00111100110 0000_0110 0011_0110  0111_1001 1100_1010 ...
        // 0001101100101000101101011100111111011010100111000001100001101011001101100111111000100111000111100110

        let mut refvec = refnum.to_be_bytes().to_vec();
        refvec.push(6); // checksum

        let mut data = Vec::<u8>::new();
        let mut bucket = 0u32;
        let mut bits_in_bucket = 0;
        for index in indices {
            // add bits to bucket
            bucket = (bucket << 11) | index;
            bits_in_bucket += 11;

            while bits_in_bucket >= 8 {
                // extract the top 8 bits from the bucket, put it into the result vector
                data.push((bucket >> (bits_in_bucket - 8)) as u8);
                // mask off the "used up" bits
                bucket &= !(0b1111_1111 << bits_in_bucket - 8);

                // subtract the used bits out of the bucket
                bits_in_bucket -= 8;
            }
        }
        if bits_in_bucket != 0 {
            data.push(bucket as u8);
        }
        assert!(data.len() == refvec.len());
        for (index, (&a, &b)) in refvec.iter().zip(data.iter()).enumerate() {
            if a != b {
                println!("index {} error: a[{}{:x})] != b[{}({:x})]", index, a, a, b, b);
            } else {
                println!("index {} match: a[{}({:x})] == b[{}({:x})]", index, a, a, b, b);
            }
            assert!(a == b);
        }
    }
    #[test]
    fn test_bip39_to_bytes() {
        let phrase = vec![
            "alert".to_string(),
            "record".to_string(),
            "income".to_string(),
            "curve".to_string(),
            "mercy".to_string(),
            "tree".to_string(),
            "heavy".to_string(),
            "loan".to_string(),
            "hen".to_string(),
            "recycle".to_string(),
            "mean".to_string(),
            "devote".to_string(),
        ];
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
        let mut refvec = refnum.to_be_bytes().to_vec();
        // refvec.push(6); // checksum

        assert_eq!(Ok(refvec), bip39_to_bytes(&phrase));
    }
    #[test]
    fn test_is_valid_bip39() {
        assert_eq!(is_valid_bip39("alert"), true);
        assert_eq!(is_valid_bip39("rEcOrD"), true);
        assert_eq!(is_valid_bip39("foobar"), false);
        assert_eq!(is_valid_bip39(""), false);
    }
    #[test]
    fn test_suggest_prefix() {
        let suggestions = suggest_bip39("ag");
        let reference =
            vec!["again".to_string(), "age".to_string(), "agent".to_string(), "agree".to_string()];
        assert_eq!(suggestions, reference);
    }
    #[test]
    fn test_bytes_to_bip39() {
        let refnum = 0b00000110001101100111100111001010000110110010100010110101110011111101101010011100000110000110101100110110011111100010011100011110u128;
        let refvec = refnum.to_be_bytes().to_vec();
        let phrase = vec![
            "alert".to_string(),
            "record".to_string(),
            "income".to_string(),
            "curve".to_string(),
            "mercy".to_string(),
            "tree".to_string(),
            "heavy".to_string(),
            "loan".to_string(),
            "hen".to_string(),
            "recycle".to_string(),
            "mean".to_string(),
            "devote".to_string(),
        ];
        assert_eq!(bytes_to_bip39(&refvec), Ok(phrase));
    }
}
