pub(crate) const SERVER_NAME_TRNG: &str = "_TRNG manager_"; // depended upon by getrandom, do not change

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct ExcursionTest {
    pub min: u16,
    pub max: u16,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct MiniRunsTest {
    pub run_count: [u16; 4],
    pub fresh: bool,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct NistTests {
    pub adaptive_b: u16,
    pub repcount_b: u16,
    pub fresh: bool,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct HealthTests {
    pub av_excursion: [ExcursionTest; 2],
    pub av_nist: [NistTests; 2],
    pub ro_miniruns: [MiniRunsTest; 4],
    pub ro_nist: [NistTests; 4],
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct TrngErrors {
    pub excursion_errs: [Option<ExcursionTest>; 2],
    pub av_repcount_errs: Option<u8>,
    pub av_adaptive_errs: Option<u8>,
    pub ro_repcount_errs: Option<u8>,
    pub ro_adaptive_errs: Option<u8>,
    pub nist_errs: u32,
    pub server_underruns: u16,
    pub kernel_underruns: u16,
    pub pending_mask: u32,
}

/// Performance issue just noticed: the data field is exactly 4096 bytes long, which means
/// the "len" field overflows the structure to be 2 pages. This will cause a lot of extra
/// zero-ing of pages, thrashing the cache and also pegging the CPU for useless work.
/// Consider revising the data field down to 1023 words in length, but need to revisit the
/// library implemnetations to make sure this doesn't break any existing code.
/// Note that this structure had to be mirrored into the local "getrandom" implementation
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TrngBuf {
    pub data: [u32; 1024],
    pub len: u16,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get one or two 32-bit words of TRNG data
    GetTrng,

    /// Fill a buffer with random data
    FillTrng,

    /// Suspend/resume callback
    SuspendResume,

    /// Notification of an error from the interrupt handler
    ErrorNotification,

    /// Subscribe to error notifications
    ErrorSubscribe,

    /// Get TRNG health stats
    HealthStats,

    /// Get Error stats
    ErrorStats,

    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum EventCallback {
    Event,
    Drop,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32, // ID of the scalar message to send through (e.g. the discriminant of the Enum on the caller's side API)
    pub cid: xous::CID, // caller-side connection ID for the scalar message to route to. Created by the caller before hooking.
}
