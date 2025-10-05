// These define the start region of the partition. In general, each partition has signature
// metadata in them, but the first word in the signature is a jump instruction that takes you
// to the actual boot code.
pub const BOOT0_START: usize = 0x6000_0000;
pub const BOOT1_START: usize = 0x6002_0000;
pub const LOADER_START: usize = 0x6006_0000;
pub const BAREMETAL_START: usize = LOADER_START;
// kernel needs to start on a page boundary, so eat into the loader area a bit to allow that to happen.
pub const KERNEL_START: usize = 0x6008_0000 - crate::signatures::SIGBLOCK_LEN;

// total storage area available in RRAM. Above this are reserved vectors for security apparatus.
pub const RRAM_STORAGE_LEN: usize = 0x3D_A000;

// loadable swap "starts" at these address for UF2 updates. They are are interpreted as
// zero-offsets from their respective "partitions" after masking for the top address location.
pub const SWAP_START_UF2: usize = 0x7000_0000;
pub const SWAP_UF2_LEN: usize = 0x0800_0000; // 128 MiB

// Define a trait with just the offset
pub trait OneWayEncoding: TryFrom<u32> {
    const OFFSET: usize;
}

macro_rules! encode_oneway {
    (
        #[offset = $offset:literal]
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $variant:ident
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u32)]
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum $name {
            $(
                $variant
            ),*
        }

        impl $name {
            const ALL: &'static [Self] = &[
                $(Self::$variant),*
            ];
        }

        impl TryFrom<u32> for $name {
            type Error = ();

            fn try_from(value: u32) -> Result<Self, Self::Error> {
                let index = (value % Self::ALL.len() as u32) as usize;
                Ok(Self::ALL[index])
            }
        }

        impl OneWayEncoding for $name {
            const OFFSET: usize = $offset;
        }
    };
}

encode_oneway! {
    #[offset = 80]
    pub enum BootWaitCoding {
        Disable,
        Enable,
    }
}

encode_oneway! {
    #[offset = 81]
    pub enum BoardTypeCoding {
        Baosec,
        Dabao,
        Oem,
    }
}

encode_oneway! {
    #[offset = 82]
    pub enum AltBootCoding {
        PrimaryPartition,
        AlternatePartition,
    }
}

pub const PUBKEY_SLOTS: usize = 4;
/// Offset in the one-way counter array for loader key revocations. Provisions for up to four
/// key slots, from [116..=120].
pub const LOADER_REVOCATION_OFFSET: usize = 116;
/// Offset in the one-way counter array for boot1 key revocations. Provisions for up to four
/// key slots, from [120..=123].
pub const BOOT1_REVOCATION_OFFSET: usize = LOADER_REVOCATION_OFFSET + PUBKEY_SLOTS;
/// Offset in the one-way counter array for boot0 key revocations. Provisions for up to four
/// key slots, from [124..=127].
pub const BOOT0_REVOCATION_OFFSET: usize = BOOT1_REVOCATION_OFFSET + PUBKEY_SLOTS;
