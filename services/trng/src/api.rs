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

/// These opcode numbers are partially baked into the `getrandom` library --
/// which kind of acts as a `std`-lib-ish style interface for the trng, so,
/// by design it can't have a dependency on this crate :-/
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get one or two 32-bit words of TRNG data
    GetTrng = 0,

    /// Fill a buffer with random data
    FillTrng = 1,

    /// Suspend/resume callback
    SuspendResume = 2,

    /// Notification of an error from the interrupt handler
    ErrorNotification = 3,

    /// Subscribe to error notifications
    ErrorSubscribe = 4,

    /// Get TRNG health stats
    HealthStats = 5,

    /// Get Error stats
    ErrorStats = 6,

    Quit = 7,

    /// Set test mode to `RngTestMode`. System will best-effort provide "normal" TRNG data
    /// but the TRNG will block while the generators are switched into test modes during buffer refills
    TestSetMode = 8,

    /// Get test data. Fails (returns no data) if test mode was not previously set.
    TestGetData = 9,
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

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq, Copy, Clone)]
pub enum TrngTestMode {
    // No test mode configured.
    None,
    // Avalanche data only
    Av,
    // Ring oscillator data only
    Ro,
    // Combined RO + AV data without any CPRNG conditioning
    Both,
    // Output of the CPRNG that is constantly re-seeded by RO + AV data. This is the "normal" mode of operation.
    // The CPRNG serves as a "belt and suspenders" safety measure over raw RO + AV data, so that small drop-outs
    // in the TRNG don't lead to disastrous consequences. Of course this also masks large failures, but there are
    // online tests that should help to pick that up.
    Cprng,
}

pub const TRNG_TEST_BUF_LEN: usize = 2048;
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TrngTestBuf {
    pub data: [u8; TRNG_TEST_BUF_LEN],
    pub len: u16,
}
