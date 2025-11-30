// Based on code from `passwords` 3.1.16 crate (MIT License, Ron Li <magiclen.org>)
//
// Reason for vendoring: needed to modify the algorithm to select *which* special characters are
// allowed. The reference implementation picks from a static const array of special characters.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

pub const DEFAULT_LENGTH: usize = 18;

static NUMBERS: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];

static LOWERCASE_LETTERS: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
    'v', 'w', 'x', 'y', 'z',
];

static UPPERCASE_LETTERS: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U',
    'V', 'W', 'X', 'Y', 'Z',
];

// All symbols we could allow. Ordered from most likely to least likely to be allowed by banks.
// Ironically, it's the banks that have the most restrictions on special charactters
pub static SYMBOLS_ALL: [char; 32] = [
    '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '_', '+', '=', '?', ',', '.', ':', ';', '[', ']',
    '{', '|', '}', '/', '\'', '"', '\\', '<', '>', '`', '~',
];

#[rustfmt::skip]
// First 8 symbols are the most likely to be allowed, so mark them by default as the set to be included.
const SYMBOLS_DEFAULT_ALLOW: [bool; 32] = [
    true, true, true, true, true, true, true, true,
    false, false, false, false, false, false, false, false,
    false, false, false, false, false, false, false, false,
    false, false, false, false, false, false, false, false,
];

#[derive(Copy, Clone, Debug)]
pub struct GeneratorConfig {
    pub length: usize,
    pub numbers: bool,
    pub upper: bool,
    pub lower: bool,
    pub use_symbols: bool,
    pub symbols: [bool; 32],
}
impl GeneratorConfig {
    pub fn default() -> Self {
        GeneratorConfig {
            length: DEFAULT_LENGTH,
            numbers: true,
            upper: true,
            lower: true,
            use_symbols: true,
            symbols: SYMBOLS_DEFAULT_ALLOW,
        }
    }

    /// Serialize into a storage string
    /// **Format example**:
    ///
    /// true
    /// false
    /// true
    /// 10101000000000000000000000000000
    pub fn serialize(&self, mut file: File) -> io::Result<()> {
        writeln!(file, "{}", self.length)?;
        writeln!(file, "{}", self.numbers)?;
        writeln!(file, "{}", self.upper)?;
        writeln!(file, "{}", self.lower)?;
        writeln!(file, "{}", self.use_symbols)?;
        log::info!("{:?}", self.symbols);
        for &sym in &self.symbols {
            write!(file, "{}", if sym { '1' } else { '0' })?;
        }
        writeln!(file)?;

        Ok(())
    }

    /// Deserialize from a storage string
    pub fn deserialize(file: File) -> io::Result<Self> {
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let length = lines.next().and_then(|r| r.ok()).and_then(|s| s.parse().ok()).unwrap_or(18usize);
        let numbers = lines.next().and_then(|r| r.ok()).and_then(|s| s.parse().ok()).unwrap_or(true);
        let upper = lines.next().and_then(|r| r.ok()).and_then(|s| s.parse().ok()).unwrap_or(true);
        let lower = lines.next().and_then(|r| r.ok()).and_then(|s| s.parse().ok()).unwrap_or(true);
        let use_symbols = lines.next().and_then(|r| r.ok()).and_then(|s| s.parse().ok()).unwrap_or(true);

        let mut symbols = [false; 32];
        if let Some(Ok(sym_line)) = lines.next() {
            for (i, c) in sym_line.chars().enumerate() {
                if i >= 32 {
                    break;
                }
                symbols[i] = c == '1';
            }
        }

        Ok(Self { length, numbers, upper, lower, use_symbols, symbols })
    }

    pub fn to_pools(&self) -> Vec<Vec<char>> {
        let mut pools = Vec::new();
        if self.lower {
            pools.push(LOWERCASE_LETTERS.into());
        }
        if self.upper {
            pools.push(UPPERCASE_LETTERS.into());
        }
        if self.numbers {
            pools.push(NUMBERS.into());
        }
        if self.use_symbols {
            let mut specials = Vec::<char>::new();
            for (i, &allow) in self.symbols.iter().enumerate() {
                if allow {
                    specials.push(SYMBOLS_ALL[i])
                }
            }
            if specials.len() > 0 {
                pools.push(specials);
            }
        }
        pools
    }
}

/// Generate random passwords. `strict` means that at least one item must be included from each of the pools.
pub fn generate(pools: &Vec<Vec<char>>, length: usize, strict: bool) -> String {
    let pool: Vec<char> = pools.iter().flat_map(|v| v.iter().copied()).collect();
    let weights = vec![1; length];
    let random = random_pick::pick_multiple_from_slice(&pool, &weights, length);

    if strict {
        let mut password = String::with_capacity(length);

        let handle = |random: &[&char], password: &mut String, pools: &Vec<Vec<char>>| {
            let mut mask = 0;
            let target_mask = (1 << pools.len()) - 1;
            let mut m = false;

            for &c in random[..].iter() {
                password.push(*c);

                if !m {
                    for (i, pool) in pools.iter().enumerate() {
                        if pool.contains(c) {
                            mask |= 1 << i;
                        }
                    }
                    m = mask == target_mask;
                }
            }

            m
        };

        if !handle(&random, &mut password, pools) {
            loop {
                let random = random_pick::pick_multiple_from_slice(&pool, &weights, length);

                password.clear();
                if handle(&random, &mut password, pools) {
                    break;
                }
            }
        }

        password
    } else {
        random.iter().map(|c| **c).collect()
    }
}
