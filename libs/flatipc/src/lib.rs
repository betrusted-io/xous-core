//! Xous supports sending Messages from Clients to Servers. If a message is
//! a `MemoryMessage`, then the Server may respond by updating the buffer
//! with a response message and returning the buffer.
//!
//! A number of serialization options are available, ranging from sending
//! raw arrays of bytes all the way to sending full Protobuf messages.
//!
//! This crate takes a middle road and allows for sending rich Rust types
//! such as full enums and structs without doing extensive checks. This is
//! based on the theory that in normal operating systems, ABIs do not contain
//! any sort of verification, and it is undefined behaviour to send a malformed
//! request to a loaded module.
//!
//! An object can be made into an IPC object by implementing the `Ipc` trait.
//! The primary method of doing this is by adding `#[derive(flatipc::Ipc)]`
//! to the definition. Such an object may be included in both the Client and
//! the Server so that they share the same view of the object.
//!
//! Any object may be made into an IPC object provided it follows the following
//! requirements:
//!
//! - **The object is `#[repr(C)]`** - This is required to ensure the objecty has a well-defined layout in
//!   memory. Other representations may shift depending on optimizations.
//! - **The object only contains fields that are `IpcSafe`** - This trait is implemented on primitive types
//!   that are able to be sent across an IPC boundary. This includes all integers, floats, and booleans. It
//!   also includes arrays of `IpcSafe` types, `Option<T>` where `T` is `IpcSafe`, and `Result<T, E>` where
//!   `T` and `E` are `IpcSafe`. Pointers and references are not `IpcSafe` and may not be used.
//!
//! When deriving `Ipc`, a new type will be created with the same name as
//! the original type prefixed with `Ipc`. For example, if you derive `Ipc`
//! on a type named `Foo`, the new type will be named `IpcFoo`.
//!
//! Objects that implement `Ipc` must be page-aligned and must be a full multiple
//! of a page size in length. This is to ensure that the object can be mapped
//! transparently between the Client and the Server without dragging any extra
//! memory along with it.
//!
//! `Ipc` objects implement `Deref` and `DerefMut` to the original object, so
//! they can be used in place of the original object in most cases by derefencing
//! the `Ipc` object.
//!
//! Because `String` and `Vec` contain pointers, they are not `IpcSafe`. As such,
//! replacements are made available in this crate that are `IpcSafe`.
//!
//! <br>
//!
//! # Example of a Derived Ipc Object
//!
//! ```ignore
//! #[repr(C)]
//! #[derive(flatipc::Ipc)]
//! pub struct Foo {
//!   a: u32,
//!   b: u64,
//! }
//!
//! // Connect to an example server
//! let conn = xous::connect(xous::SID::from_bytes(b"example---server").unwrap())?;
//!
//! // Construct our object
//! let foo = Foo { a: 1, b: 1234567890 };
//!
//! // Create an IPC representation of the original object, consuming
//! // the original object in the process.
//! let mut foo_ipc = foo.into_ipc();
//!
//! // Lend the object to the server with opcode 42.
//! foo_ipc.lend(conn, 42)?;
//!
//! // Note that we can still access the original object.
//! println!("a: {}", foo_ipc.a);
//!
//! // When we're done with the IPC object, we can get the original object back.
//! let foo = foo_ipc.into_original();
//! ```
//!
//! # Example of Server Usage
//!
//! ```ignore
//! // Ideally this comes from a common `API` file that both the
//! // client and server share.
//! #[repr(C)]
//! #[derive(flatipc::Ipc)]
//! pub struct Foo {
//!   a: u32,
//!   b: u64,
//! }
//!
//! let mut msg_opt = None;
//! let mut server = xous::create_server_with_sid(b"example---server").unwrap();
//! loop {
//!     let envelope = xous::reply_and_receive_next(server, &mut msg_opt).unwrap();
//!     let Some(msg) = msg_opt else { continue };
//!
//!     // Take the memory portion of the message, continuing if it's not a memory message.
//!     let Some(msg_memory) = msg.memory_message() else { continue };
//!
//!     // Turn the `MemoryMessage` into an `IpcFoo` object. Note that this is the `Ipc`-prefixed
//!     // version of the original object. If the object is not the correct type, `None` will be
//!     // returned and the message will be returned to the sender in ghe next loop.
//!     let Some(foo) = IpcFoo::from_memory_message(msg_slice, signature) else { continue };
//!
//!     // Do something with the object.
//! }
//! ```

/// An object is Sendable if it is guaranteed to be flat and contains no pointers.
/// This trait can be placed on objects that have invalid representations such as
/// bools (which can only be 0 or 1) but it is up to the implementer to ensure that
/// the correct object arrives on the other side.
pub unsafe trait IpcSafe {}

// Enable calling this crate as `flatipc` in tests.
extern crate self as flatipc;

// Allow doing `#[derive(flatipc::Ipc)]` instead of `#[derive(flatipc_derive::Ipc)]`
pub use flatipc_derive::{Ipc, IpcSafe};
#[cfg(feature = "xous")]
mod backend {
    pub use xous::CID;
    pub use xous::Error;
}

#[cfg(not(feature = "xous"))]
mod backend {
    pub mod mock;
    pub use mock::CID;

    #[derive(Debug)]
    pub enum Error {
        Unimplemented,
    }
}

pub use backend::{CID, Error};

pub mod string;
pub use string::String;

pub mod vec;
pub use vec::Vec;

unsafe impl IpcSafe for i8 {}
unsafe impl IpcSafe for i16 {}
unsafe impl IpcSafe for i32 {}
unsafe impl IpcSafe for i64 {}
unsafe impl IpcSafe for i128 {}
unsafe impl IpcSafe for u8 {}
unsafe impl IpcSafe for u16 {}
unsafe impl IpcSafe for u32 {}
unsafe impl IpcSafe for u64 {}
unsafe impl IpcSafe for u128 {}
unsafe impl IpcSafe for f32 {}
unsafe impl IpcSafe for f64 {}
unsafe impl IpcSafe for bool {}
unsafe impl IpcSafe for usize {}
unsafe impl IpcSafe for isize {}
unsafe impl IpcSafe for char {}
unsafe impl<T, const N: usize> IpcSafe for [T; N] where T: IpcSafe {}
unsafe impl<T> IpcSafe for Option<T> where T: IpcSafe {}
unsafe impl<T, E> IpcSafe for Result<T, E>
where
    T: IpcSafe,
    E: IpcSafe,
{
}

/// An object that can be sent across an IPC boundary, and can be reconstituted
/// on the other side without copying. An object with this trait must be page-aligned,
/// must be a multiple of the page size in length, and must not contain any pointers.
pub unsafe trait Ipc {
    /// What this memory message is a representation of. This is used to turn
    /// this object back into the original object.
    type Original;

    /// Create an Ipc variant from the original object. Succeeds only if
    /// the signature passed in matches the signature of `Original`.
    fn from_slice<'a>(data: &'a [u8], signature: usize) -> Option<&'a Self>;

    /// Unconditionally create a new memory message from the original object.
    /// It is up to the caller to that `data` contains a valid representation of `Self`.
    unsafe fn from_buffer_unchecked<'a>(data: &'a [u8]) -> &'a Self;

    /// Create a mutable IPC variant from the original object. Succeeds only if
    /// the signature passed in matches the signature of `Original`.
    fn from_slice_mut<'a>(data: &'a mut [u8], signature: usize) -> Option<&'a mut Self>;

    /// Unconditionally create a new mutable memory message from the original object.
    /// It is up to the caller to that `data` contains a valid representation of `Self`.
    unsafe fn from_buffer_mut_unchecked<'a>(data: &'a mut [u8]) -> &'a mut Self;

    /// Return a reference to the original object while keeping the
    /// memory version alive.
    fn as_original(&self) -> &Self::Original;

    /// Return a reference to the original object while keeping the
    /// memory version alive.
    fn as_original_mut(&mut self) -> &mut Self::Original;

    /// Consume the memory version and return the original object.
    fn into_original(self) -> Self::Original;

    /// Lend the buffer to the specified server. The connection should already be
    /// open and the server should be ready to receive the buffer.
    fn lend(&self, connection: CID, opcode: usize) -> Result<(), backend::Error>;

    /// Try to lend the buffer to the specified server, returning an error
    /// if the lend failed.
    fn try_lend(&self, connection: CID, opcode: usize) -> Result<(), backend::Error>;

    /// Lend the buffer to the specified server, and allow the server to
    /// modify the buffer.
    fn lend_mut(&mut self, connection: CID, opcode: usize) -> Result<(), backend::Error>;

    /// Lend the buffer to the specified server, and allow the server to
    /// modify the buffer. Return an error if the lend failed.
    fn try_lend_mut(&mut self, connection: CID, opcode: usize) -> Result<(), backend::Error>;

    /// Return the signature of this memory message. Useful for verifying
    /// that the correct message is being received.
    fn signature(&self) -> usize;

    #[cfg(feature = "xous")]
    /// Build an `Ipc` object from a `xous::MemoryMessage`. Verifies the signature and
    /// returns `None` if there is no match.
    fn from_memory_message<'a>(msg: &'a xous::MemoryMessage) -> Option<&'a Self>;

    #[cfg(feature = "xous")]
    /// Build a mutable `Ipc` object from a mutable `xous::MemoryMessage`. Verifies the
    /// signature and returns `None` if there is no match. The returned object has a
    /// lifetime that's tied to the `MemoryMessage`.
    fn from_memory_message_mut<'a>(msg: &'a mut xous::MemoryMessage) -> Option<&'a mut Self>;
}

/// Objects that have `IntoIpc` may be turned into an object that can be passed
/// across an IPC barrier. This consumes the object and returns a new object that
/// may be dereferenced to the original object.
pub trait IntoIpc {
    type IpcType;
    fn into_ipc(self) -> Self::IpcType;
}

#[cfg(test)]
mod test;
