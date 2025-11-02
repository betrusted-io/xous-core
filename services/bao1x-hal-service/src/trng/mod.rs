#[cfg(feature = "board-baosec")]
pub mod baosec;
#[cfg(feature = "board-baosec")]
pub use baosec::*;
#[cfg(feature = "board-dabao")]
pub mod dabao;
#[cfg(feature = "board-dabao")]
pub use dabao::*;

pub mod api {
    pub const SERVER_NAME_TRNG: &str = "_TRNG manager_"; // depended upon by getrandom, do not change

    /// These opcode numbers are partially baked into the `getrandom` library --
    /// which kind of acts as a `std`-lib-ish style interface for the trng, so,
    /// by design it can't have a dependency on this crate :-/
    #[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
    pub enum Opcode {
        /// Get one or two 32-bit words of TRNG data
        GetTrng = 0,

        /// Fill a buffer with random data
        FillTrng = 1,

        /// Notification of an error from the interrupt handler
        ErrorNotification = 3,

        /// Subscribe to error notifications
        ErrorSubscribe = 4,

        /// Get TRNG health stats
        HealthStats = 5,

        /// Get Error stats
        ErrorStats = 6,
    }

    #[derive(Debug, flatipc::Ipc)]
    #[allow(dead_code)]
    /// Note that this structure is mirrored in imports/getrandom/src/xous.rs
    #[repr(C)]
    pub struct TrngBuf {
        pub data: [u32; 1020],
        pub len: u16,
    }

    #[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
    pub enum EventCallback {
        Event,
        Drop,
    }

    #[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
    pub struct ScalarHook {
        pub sid: (u32, u32, u32, u32),
        pub id: u32, /* ID of the scalar message to send through (e.g. the discriminant of the Enum on the
                      * caller's side API) */
        pub cid: xous::CID, /* caller-side connection ID for the scalar message to route to. Created by
                             * the caller before hooking. */
    }

    #[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
    pub struct NistTests {
        pub adaptive_b: u16,
        pub repcount_b: u16,
        pub fresh: bool,
    }

    #[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
    pub struct HealthTests {
        pub av_nist: NistTests,
        pub ro_nist: NistTests,
    }

    #[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
    pub struct TrngErrors {
        pub nist_errs: u32,
        pub pending_mask: u32,
    }
}
