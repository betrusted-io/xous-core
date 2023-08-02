pub(crate) const SERVER_NAME_APP_LOADER: &str = "_App Loader_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// to load an app from usb into ram
    LoadApp,

    /// gets the app data for the given index
    FetchAppData,

    /// dispatch the app
    DispatchApp,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Return {
    Failure,
    Info(App),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct App {
    pub name: xous_ipc::String<64>,
//    pub token: [u32;4]
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct AppRequest {
    pub index: usize,
    pub auth: Option<[u32;4]>
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct LoadAppRequest {
    pub name: xous_ipc::String<64>
}
