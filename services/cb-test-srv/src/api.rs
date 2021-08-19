pub(crate) const SERVER_NAME: &str = "_Callback test server_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    RegisterTickListener,
    RegisterReqListener,
    UnregisterReqListener,
    Tick,
    Req,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TickCallback {
    Tick,
    Drop,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ResultCallback {
    Result,
    Drop,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32, // ID of the scalar message to send through (e.g. the discriminant of the Enum on the caller's side API)
    pub cid: xous::CID, // caller-side connection ID for the scalar message to route to. Created by the caller before hooking.
}
