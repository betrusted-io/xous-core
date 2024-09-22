use std::sync::{LazyLock, Mutex};
// Make a CID a u128 just to be different from Xous and ensure
// the types don't make assumptions.
pub type CID = u128;

pub struct Server {
    lend: Box<dyn Send + Fn(usize, usize, usize, &[u8]) -> (usize, usize)>,
    lend_mut: Box<dyn Send + Fn(usize, usize, usize, &mut [u8]) -> (usize, usize)>,
}

impl Server {
    pub fn new(
        lend: Box<dyn Send + Fn(usize, usize, usize, &[u8]) -> (usize, usize)>,
        lend_mut: Box<dyn Send + Fn(usize, usize, usize, &mut [u8]) -> (usize, usize)>,
    ) -> Self {
        Server { lend, lend_mut }
    }
}

pub struct IpcMachine {
    servers: Vec<Server>,
}

pub(crate) static IPC_MACHINE: LazyLock<Mutex<IpcMachine>> = LazyLock::new(|| Mutex::new(IpcMachine::new()));

impl IpcMachine {
    fn new() -> Self { IpcMachine { servers: Vec::new() } }

    pub fn add_server(&mut self, server: Server) -> CID {
        let server_id = self.servers.len() as CID;
        self.servers.push(server);
        server_id
    }

    pub fn lend(&self, server_id: CID, opcode: usize, a: usize, b: usize, data: &[u8]) {
        let server_id = server_id as usize;
        (self.servers[server_id].lend)(opcode, a, b, data);
    }

    pub fn lend_mut(&self, server_id: CID, opcode: usize, a: usize, b: usize, data: &mut [u8]) {
        let server_id = server_id as usize;
        (self.servers[server_id].lend_mut)(opcode, a, b, data);
    }

    pub fn try_lend(&self, server_id: CID, opcode: usize, a: usize, b: usize, data: &[u8]) {
        self.lend(server_id, opcode, a, b, data);
    }

    fn try_lend_mut(&self, server_id: CID, opcode: usize, a: usize, b: usize, data: &mut [u8]) {
        self.lend_mut(server_id, opcode, a, b, data);
    }
}
