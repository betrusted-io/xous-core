pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {

    /// Suspend/resume callback
    SuspendResume,
}

// this is an intenal structure for managing the overall PDDB
pub(crate) struct PddbManager {

}
impl PddbManager {
    // return a list of open bases
    fn list_basis();
}

// this is an internal struct for managing a basis
pub(crate) struct PddbBasis {

}
impl PddbBasis {
    // opening and closing basis will side-effect PddbDicts and PddbKeys
    fn open(); // will result in a password box being triggered to open a basis
    fn close();
}

// this structure can be shared on the user side?
pub struct PddbDict {
    contents: Hashmap,
}
impl PddbDict {
    // opens a dictionary only if it exists
    pub fn open(dict_name: &str) -> Option<PddbDict>;
    // creates a dictionary only if it does not already exist
    pub fn create(dict_name: &str) -> Option<PddbDict>;

    // returns a key only if it exists
    pub fn get(&mut self, key_name: &str, key_changed_cb: CB) -> Result<PddbKey>;
    // updates an existing key's value. mainly used by write().
    pub fn update(&mut self, key: PddbKey) -> Result<Option<PddbKey>>;
    // creates a key or overwrites it
    pub fn insert(&mut self, key: PddbKey) -> Option<PddbKey>; // may return the displaced key
    // deletes a key within the dictionary
    pub fn remove(&mut self, key: PddbKey) -> Result<()>;
    // deletes the entire dictionary
    pub fn delete(&mut self);
}

/// PddbKey is somewhat isomorphic to a File in Rust, in that it provides slices of [u8] that
/// can be `read()`, `write()` and `seek()`.
/// this is definitely a user-facing structure
pub struct PddbKey<CB>
where CB: FnMut(), {
    // dictionary to search for the key within
    dict: PddbDict,
    // a copy of my name
    name: String,
    // called when the key changes (basis or is modified otherwise)
    key_changed_cb: CB,
    // mapped memory for the plaintext contexts, typically not all resident
    mem: MemoryRange,
}

impl PddbKey<'a, CB> {
    // reads are transparent and "just happen"
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    // writes will call update() on the dictionary
    pub fn write(&mut self, buf: &[u8]) -> Result<usize>;
    // provided for compatibility with Rust API
    pub fn seek(&mut self, pos: SeekFrom) -> Result<u64>;
}

pub struct PddbBasis {

}