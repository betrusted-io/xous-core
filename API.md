The [Xous Book](https://betrusted.io/xous-book/ch07-02-caller-idioms.html) reviews the system architecture and breaks down the various API idioms chapter-by-chapter.

This document conveys much of the similar information, but in a more monolithic form. This document also does not cover [deferred response](https://betrusted.io/xous-book/ch07-06-deferred.html) idioms.

## Xous Messaging API In Practice

All Messages passed between Xous Servers undergo serialization and de-serialization.
- Internal `struct` are readily serialized with [rkyv](https://docs.rs/rkyv/0.4.3/rkyv/) and `#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]`with manual serialization as an alternative ([example](services/net/src/std_udp.rs))
- External `struct` may employ [bincode](https://docs.rs/bincode/latest/bincode/)

Here are the idioms for building servers and passing messages.

### api.rs

Incoming messages are defined in an `Opcode` enum, by convention
```rust
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
   ExampleScalar,
   ExampleBlockingScalar,
   ExampleMemory,
   ExampleMemoryWithReturn,
   RegisterCallback,
   UnregisterCallback,
}
```

Synchronous return messages are defined in a `Return` enum, by convention
```rust
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Return {
    ExampleMemoryReturn(RichMemStruct),
    Failure,
}
```

Asynchronous callback messages are defined in a `Callback` enum, by convention
```rust
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Callback {
    Hello,
    Drop,
}
```

Rich memory structures for IPC are also defined in the api.rs crate. These may or may not
be scoped to crate-local.
```rust
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct RichMemStruct {
    pub name: String,
    pub stuff: [u32; 42],
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AnotherRichStruct {
    pub name: String,
    pub other: Option<bool>,
}
```

A server name is also defined in api.rs. This is a human-readable, 64-byte
description that uniquely identifies your server to the name server. This is
later mapped to a random 64-bit ID that only the name server knows.

```rust
pub(crate) const MY_SERVER_NAME: &str = "_Example server_"; // the underscores are optional, but they help readability in logs
```

### lib.rs

Client state is held in an object defined in lib.rs.

```rust
#![cfg_attr(target_os = "none", no_std)]
pub mod api;
use api::{Callback, Opcode}; // if you prefer to map the api into your local namespace
use xous::{send_message, Error, CID, Message, msg_scalar_unpack};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

pub struct MyServer {
  conn: xous::CID,
  callback_sid: Option<xous::SID>, // this is only necessary if you have callbacks
}
impl MyServer {
  pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
    let conn = xns.request_connection_blocking(api::MY_SERVER_NAME).expect("Can't connect to MyServer");
    Ok(MyServer {
      conn,
      callback_sid: None,
    })
  }

  /// an example of a client requesting to send a message to MyServer
  /// you can pass up to four usize-args this way, this example has none and just sends 4 zeros as placeholders
  pub fn send_example_scalar(&self) -> Result<(), xous::Error> {
    send_message(self.conn,
      Message::new_scalar(Opcode::ExampleScalar.to_usize().unwrap()), 0, 0, 0, 0)
    ).map(|_|())
  }

  /// if you want some return data, a blocking scalar is the way to go
  /// in this case, we introduce two arbitrary args, `a` and `b`, and return a u32
  pub fn send_example_scalar(&self, a: u32, b: u32) -> Result<u32, xous::Error> {
    let response = send_message(self.conn,
      Message::new_blocking_scalar(Opcode::ExampleBlockingScalar.to_usize().unwrap()), a, b, 0, 0)
    ).expect("ExampleBlockingScalar failed");
    // you could also receive two scalar values if you use Scalar2 instead of Scalar1
    if let xous::Result::Scalar1(result) = response {
      Ok(result as u32)
    } else {
      log::error!("unexpected return value: {:#?}", response);
      Err(xous::Error::InternalError)
    }
  }

  /// an example of sending a rich data structure
  pub fn send_richdata(&self, words: &str, stuff: [u32; 42]) -> Result<(), xous::Error> {
    // build the structure up. Note that RichMemStruct is just inside the API! the caller doesn't need to know about it.
    let mut rich_struct = RichMemStruct {
      name: String::new(),
      stuff,
    };
    use core::fmt::Write;
    write!(rich_struct.name, "{}", words).expect("words too long");
    // now convert it into a Xous::Buffer, which can then be lent to the server
    let buf = Buffer::into_buf(rich_struct).or(Err(xous::Error::InternalError))?;
    buf.lend(self.conn, Opcode::ExampleMemory.to_u32().unwrap()).map(|_| ())
  }

  /// an example of rich data with a return type
  pub fn get_richdata(&self, stuff: [u32; 42]) -> Result<AnotherRichStruct, xous::Error> {
    // build the query up. We're going to re-use RichMemStruct, but it could be anything
    let mut rich_struct = RichMemStruct {
      name: String::from("example rich query"),
      stuff,
    };
    // now convert it into a Xous::Buffer, which can then be mutably lent to the server
    let mut buf = Buffer::into_buf(rich_struct).or(Err(xous::Error::InternalError))?;
    buf.lend_mut(self.conn, Opcode::ExampleMemoryWithReturn.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

    // note that to_original() creates a local copy on the stack of the returned buffer
    // if you just need to access fields, you can use to_flat() which is a
    // zerocopy operation on an "Archived" version of your structure
    match buf.to_original().unwrap() {
      api::Return::ExampleMemoryReturn(rms) => {
        Ok( AnotherRichStruct {
          name: String::from(rms.name),
          other: None,
        } )
      }
      api::Return::Failure => {
        Err(xous::Error::InternalError)
      }
      _ => panic!("Got unknown return code")
    }
  }

  /// an example of registering for a callback
  /// Note: since Xous 0.9 we support `std` threads and closures. See the Net crate or
  /// Rtc lib implementation (llio/src/rtc_lib.rs)
  /// for an example of how to implement callbacks using `std` primitives.
  /// The below implementation is how closures are done in a `no-std` fashion. This is
  /// considered deprecated.
  static mut MYSERVER_CB: Option<fn(BattStats)> = None; // this actually goes outside the object decl
  pub fn hook_callback(&mut self, cb: fn(u32)) -> Result<(), xous::Error> {
      if unsafe{MYSERVER_CB}.is_some() {
          return Err(xous::Error::MemoryInUse) // can't hook it twice
      }
      let sid_tuple = (u32, u32, u32, u32);
      unsafe{MYSERVER_CB = Some(cb)};
      if let Some(sid) = self.callback_sid {
        sid_tuple = sid.to_u32();
      } else {
        let sid = xous::create_server().unwrap();
        self.callback_sid = Some(sid);
        sid_tuple = sid.to_u32();
        xous::create_thread_4(callback_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
      }
      xous::send_message(self.conn,
          Message::new_scalar(Opcode::RegisterCallback.to_usize().unwrap(),
          sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
      )).unwrap();
      Ok(())
  }
  pub fn unhook_callback(&mut self) -> Result<(), xous::Error> {
    unsafe{MYSERVER_CB = None};
    if let Some(sid) = self.callback_sid {
      let sid_tuple = sid.to_u32();
      xous::send_message(self.conn,
        Message::new_scalar(Opcode::UnregisterCallback.to_usize().unwrap(),
        sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
      )).unwrap();
    }
    Ok(())
  }
}

/// handles callback messages from server, in the library user's process space.
fn callback_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Callback::Hello) => msg_scalar_unpack!(msg, a, _, _, _, {
                unsafe {
                    if let Some(cb) = MYSERVER_CB {
                        cb(a as u32)
                    } else {
                      // this results in a race condition between the unregister message and the actual
                      // unregistration. In this case, just ignore the message.
                      continue;
                    }
                }
            }),
            Some(Callback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

impl Drop for MyServer {
    fn drop(&mut self) {
        // if we have callbacks, destroy the callback server
        if let Some(sid) = self.callback_sid.take() {
            // no need to tell the upstream server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        // all implementations will need this
        unsafe{xous::disconnect(self.conn).unwrap();}
    }
}

// reference counting implementation for servers that can be cloned or have multiple instances in a thread
//REFCOUNT.fetch_add(1, Ordering::Relaxed);
use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Codec {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}

```

### main.rs

The server implementation is in main.rs. It handles the requests coming from lib.rs.
Here is a generic template example that doesn't do very much, other than exercise the four
API cases laid out above: sending a scalar message, handling a blocking scalar (e.g. scalar
message with return value), a "lend" memory message, and a "mutable lend" memory message (e.g.
a memory message with return value).

Note that all message types block the caller until they are returned.

```rust
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use num_traits::FromPrimitive;
use xous_ipc::Buffer;
use api::Opcode;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

fn main() -> ! {
    log_server::init_wait().unwrap();
    info!("my PID is {}", xous::process::id()); // this is so we can figure out what PID goes to what server

    let xns = xous_names::XousNames::new().unwrap();
    let my_sid = xns.register_name(api::MY_SERVER_NAME).expect("can't register server");
    trace!("registered with NS -- {:?}", sid);

    // only needed if doing callbacks
    let mut cb_conns: [bool; xous::MAX_CID] = [false; xous::MAX_CID]; // 34 to hold maximum connection ID number

    loop {
      let msg = xous::receive_message(sid).unwrap(); // this blocks until we get a message
      trace!("Message: {:?}", msg);
      match FromPrimitive::from_usize(msg.body.id()) {
        Some(Opcode::ExampleScalar) => msg_scalar_unpack!(msg, _, _, _, _, { /* what the scalar message does */ }),
        Some(Opcode::ExampleBlockingScalar) => msg_blocking_scalar_unpack!(msg, a, b, _, _, {
            let value = a + b;
            xous::return_scalar(msg.sender, value).expect("couldn't return value to ExampleBlockingScalar");
            // note that you can also return two usize values with return_scalar2
        }),
        Some(Opcode::ExampleMemory) => {
          let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
          let rms = buffer.as_flat::<RichMemStruct, _>().unwrap();
          // we can now access fields in rms without copying the data
          // use to_original if you need to invoke methods on the struct object
        }
        Some(Opcode::ExampleMemoryWithReturn) => {
          let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
          let rms = buffer.to_original::<RichMemStruct, _>().unwrap();
          let response = api::Return;
          if things_look_okay {
            let retstruct = AnotherRichStruct {
              name: String::from(rms.as_str()),
              other: Some(true),
            }
            response = api::Return::ExampleMemoryReturn(retstruct);
          } else {
            response = api::Return::Failure
          }
          buffer.replace(response).unwrap(); // the buffer that was sent is replaced with AnotherRichStruct
        }
        // only needed if doing callbacks
        Some(Opcode::RegisterCallback) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
            let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
            let cid = xous::connect(sid).unwrap();
            if (cid as usize) < cb_conns.len() {
              cb_conns[cid as usize] = true;
            } else {
              error!("RegisterCallback CID out of range");
            }
          }
        ),
        Some(Opcode::UnregisterCallback) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
            let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
            let cid = xous::connect(sid).unwrap();
            if (cid as usize) < cb_conns.len() {
              cb_conns[cid as usize] = false;
            } else {
              error!("UnregisterCallback CID out of range");
            }
            unsafe{xous::disconnect(cid).unwrap()};
        })
        None => log::error!("couldn't convert opcode")
      }

      // if you have callbacks, you'll probably want to start a separate thread to handle them
      // but for simplicity we just call a function here. But in this example, it means the callback
      // can only gets processed after any message is received.
      do_callback(&mut cb_conns);
    }
}

/// only if doing callbacks. This might actually make more sense in a thread of its own, to make it
/// truly asynchronous, but this example is focused on the messaging API, not the threading API.
/// This example admittedly sweeps a rather hairy issue (passing data between process-local threads)
/// under the carpet. As of Xous 0.8 your options to send data around between threads within a process
/// are:
///  1. Define a crate-local API to pass the messages
///  2. Use Atomic data types (only applicable if you have primitive data to send)
///  3. Do some static mut unsafe thing because we don't have a Mutex data type yet.
fn do_callback(cb_conns: &mut [bool; xous::MAX_CID]) {
  let a = useful_computation();
  for cid in 1..cb_conns {
    if cb_conns[cid] {
       match xous::send_message(cid,
         xous::Message::new_scalar(api::Callback::Hello.to_usize().unwrap(), a, 0, 0, 0)
       ) {
         Err(xous::Error::ServerNotFound) => {
           cb_conns[cid] = false // automatically de-allocate callbacks for clients that have dropped
         },
         Ok(xous::Result::Ok) => {}
         _ => panic!("unhandled error or result in callback processing")
       }
    }
  }
}
```
