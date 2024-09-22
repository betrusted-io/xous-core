// As of Rust 1.64.0:
//
// Rkyv-derived enums throw warnings that rkyv::Archive derived enums are never used
// and I can't figure out how to make them go away. Since they spam the build log,
// rkyv-derived enums are now isolated to their own file with a file-wide `dead_code`
// allow on top.
//
// This might be a temporary compiler regression, or it could just
// be yet another indicator that it's time to upgrade rkyv. However, we are waiting
// until rkyv hits 0.8 (the "shouldn't ever change again but still not sure enough
// for 1.0") release until we rework the entire system to chase the latest rkyv.
// As of now, the current version is 0.7.x and there isn't a timeline yet for 0.8.

#![allow(dead_code)]
#[derive(Copy, Clone, Eq, PartialEq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum BasisRetentionPolicy {
    Persist,
    ClearAfterSleeps(u32),
    //TimeOutSecs(u32),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq)]
pub enum PddbRekeyOp {
    /// rekeys the a restored PDDB to the current device DNA using the "fast" method.
    /// The "fast" method is significantly faster on PDDBs with a small amount of data, but
    /// it will leak information on the amount of data in the PDDB, in a manner that can be
    /// trivially recovered by doing comparative ciphertext analysis between the backup and
    /// the current database image. *Some* amount of chaffe data is written, but only a
    /// small amount.
    FromDnaFast(u64),
    /// same as the above, but blank space is also turned over, guaranteeing the deniability
    /// of stored data even if an attacker has the previous backup copy of the PDDB.
    FromDnaSafe(u64),
    /// Basically the same as FromDnaSafe, but doing a self-to-self "safe" rekey
    Churn,
    /*
    // skip this implementation for now. This opcode fits generally into this code flow,
    // but requires some rework of the UX to actually acquire the old and new passwords.
    // this UX work is off-topic from the mission of getting backup restoration done,
    // but the potential to integrate the password rotation scheme into this function is
    // noted here for future efforts.
    //
    /// Requests a single secret basis to have its password changed. This will reveal the size
    /// of the Basis if the attacker has a before-and-after image of the PDDB.
    /// Recommended to call `Churn` after this operation is done for optimal safety.
    ///
    /// Note: changing the password on the .System basis is a different flow. The
    /// system basis keys are encrypted directly by the rootkeys enclave, so changing
    /// its password requires calling a routine in root_keys (that does not exist
    /// at this current time).
    ChangePass(String<BASIS_NAME_LEN>),
    */
    /// Return codes
    Success,
    AuthFail,
    UserAbort,
    VerifyFail,
    InternalError,
}
