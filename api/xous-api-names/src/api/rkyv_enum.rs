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
use crate::api::AuthenticateRequest;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[repr(C)]
pub enum Return {
    /// The caller must perform an AuthenticatedLookup using this challenge
    AuthenticateRequest(AuthenticateRequest),

    /// The connection failed for some reason
    Failure,

    /// A server was successfully created with the given SID
    SID([u32; 4]),

    /// A connection was successfully made with the given CID; an optional "disconnect token" is provided
    CID((xous::CID, Option<[u32; 4]>)),

    /// Operation requested was otherwise successful (currently only used by disconnect to ack the
    /// disconnect)
    Success,
}
