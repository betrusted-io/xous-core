# Relase 0.8 notes

This is the first release that attempts to stabilize the Xous APIs. Up
until now, everything was basically experimental and could be torn up
and recast.

At 0.8, APIs are still subject to change, but will require a higher
bar for review before incorporating the change. As the number implies,
we expect to do a backward-compatibility-breaking 0.9 API change that
incorporates a Xous `libstd` before we hit a 1.0 API milestone where
we will start preferring bodges and patches to maintain API
compatibility, over refactoring and improvement.

## Key concepts from previous Xous not changed
- IPC messages can be `scalar` or `memory`. Scalar messages are sent in
  registers. Memory messages are sent by remapping virtual pages between
  process spaces.
- A `server` is the generic term for a program that runs in its own process
  space. It can also start a `server` which can receive messages from other
  processes.
- The `server` idiom consist of a main.rs that contains the handlers for
  requests from other servers, and a lib.rs that is a set of functions other
  servers can call to make requests. api.rs is the border between lib/main,
  and such operations are limited to crate-level scope.
- Note that functions in the lib.rs run in the process space of the *caller*,
  even though the code exists in the crate of the server.

## Major features of the 0.8 API
- Migration to `rkyv` as the method for passing rich structures via IPC
- Clarification of API names to differentiate zero-copy (flat) operations.
- Migration of `String` type to a `xous-ipc` crate. Strings are kept separate
  from other data types as it opens a clean path to migrate them into `std`
- Use of enum discriminants to definitively match API calls across lib/main
  boundaries for both scalar and memory messages.
- Incorporation of `num_derive` as a core dependency of Xous. This was incorporated
  to allow us to convert API enums to u32 types and vice-versa cleanly.
- Elimination of complex enum types in the API. This means the identifiers
  no longer codify their arguments, and this binding is pushed into the
  respective lib/main implementations.
- Elimination of intra-crate API leakage. Instead of leaking a message opcode
  type outside a given server crate which callers decode in their message
  receive loop, callers that require a deferred callback instead register
  a function with the lib.rs, which is automatically invoked when the callback
  happens.
- Note that this means everytime a callback is registered, a new thread is started
  in the process space of the caller to wait for the callback message. These
  threads are low-cost (exactly 1 page of RAM) and do not burden the scheduling loop
  as they fully block until a callback message arrives.
- Encapsulation of client-side API calls to a server within an object that
  maintains variables like connection state.
- Addition of `Drop` trait to connection state objects, so when a client exits
  connections clean themselves up.
- Splitting out return message definitions from the internal API enum. Previously,
  we were re-using the API name space to define return messages. Now, there is
  a distinction between client->server messages being enumerated, by convention,
  in an `Opcode` enum in the API crate, and then potential return memory messages
  being enumerated in a `Return` enum in the API crate. The return type conventions are
  a bit more ad-hoc, though, because not all servers require them.
- Numerous fixes to the scheduler and threading API to fix latent bugs
- Upgrade logging infrastructure to handle rich logging data, including filename,
  line number, error level and so forth
- Incorporating `const_generics`, which means our minimum Rust version is 1.51
- Incorporation of `msg_scalar_unpack!()`, `msg_blocking_scalar_unpack!()`, `new_scalar()`,
  and `new_blocking_scalar()` to de-clutter code and make scalar messages a little bit
  easier to deal with

## 0.8 Messaging API In Practice

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
    pub name: xous_ipc::String::<64>,
    pub stuff: [u32; 42],
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AnotherRichStruct {
    pub name: xous_ipc::String::<64>,
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
use api::{Callback, Opcode}; // if you prefer to map the api into your local namespace
use xous::{send_message, Error, CID, Message, msg_scalar_unpack};
use xous_ipc::{String, Buffer};
use num_traits::{ToPrimitive, FromPrimitive};

pub struct MyServer {
  conn: xous::CID,
  callback_sid: Option<xous::SID>, // this is only necessary if you have callbacks
}
impl MyServer {
  pub fn new(xns: xous_names::XousNames) -> Result<Self, xous::Error> {
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
      Message::new_blocking_scalar(Opcode::ExampleScalar.to_usize().unwrap()), a, b, 0, 0)
    ).map(|_|())
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
      name: String::<64>::new(),
      stuff,
    };
    use core::fmt::Write;
    write!(rich_struct.name, "{}", words).expect("words too long");
    // now convert it into a Xous::Buffer, which can then be lent to the server
    let buf = Buffer::into_buf(rich_struct).or(Err(xous::Error:InternalError))?;
    buf.lend(self.conn, Opcode::ExampleMemory.to_u32().unwrap()).map(|_| ())
  }

  /// an example of rich data with a return type
  pub fn get_richdata(&self, stuff: [u32; 42]) -> Result<AnotherRichStruct, xous::Error> {
    // build the query up. We're going to re-use RichMemStruct, but it could be anything
    let mut rich_struct = RichMemStruct {
      name: String::<64>::from_str("example rich query"),
      stuff,
    };
    // now convert it into a Xous::Buffer, which can then be mutably lent to the server
    let mut buf = Buffer::into_buf(rich_struct).or(Err(xous::Error:InternalError))?;
    buf.lend_mut(self.conn, Opcode::ExampleMemoryWithReturn.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

    // note that to_original() creates a local copy on the stack of the returned buffer
    // if you just need to access fields, you can use to_flat() which is a
    // zerocopy operation on an "Archived" version of your structure
    match buf.to_original().unwrap() {
      api::Return::ExampleMemoryReturn(rms) => {
        Ok( AnotherRichStruct {
          name: String::<64>::from_str(rms.name),
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
  static mut MYSERVER_CB: Option<fn(BattStats)> = None;
  pub fn hook_callback(&mut self, cb: fn(u32)) -> Result<(), xous::Error> {
      if unsafe{MYSERVER_CB}.is_some() {
          return Err(xous::Error::MemoryInUse) // can't hook it twice
      }
      unsafe{MYSERVER_CB = Some(cb)};
      if self.callback_sid.is_none() {
          let sid = xous::create_server().unwrap();
          self.callback_sid = Some(sid);
          let sid_tuple = sid.to_u32();
          xous::create_thread_4(callback_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
          xous::send_message(self.conn,
              Message::new_scalar(Opcode::RegisterCallback.to_usize().unwrap(),
              sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
          )).unwrap();
      }
      Ok(())
  }
}

/// handles callback messages from the COM server, in the library user's process space.
fn callback_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Callback::Hello) => msg_scalar_unpack!(msg, a, _, _, _, {
                unsafe {
                    if let Some(cb) = MYSERVER_CB {
                        cb(a as u32)
                    }
                }
            }),
            Some(Callback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
}

impl Drop for MyServer {
    fn drop(&mut self) {
        // if we have callbacks, destroy the callback server
        if let Some(sid) = self.callback_sid.take() {
            // no need to tell the pstream server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_blocking_scalar(api::Callback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
            xous::destroy_server(sid).unwrap();
        }

        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        // all implementations will need this
        unsafe{xous::disconnect(self.conn).unwrap();}
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
use num_traits::FromPrimitive;
use xous_ipc::{String, Buffer};
use api::Opcode;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    info!("my PID is {}", xous::process::id()); // this is so we can figure out what PID goes to what server

    let xns = xous_names::XousNames::new().unwrap();
    let my_sid = xns.register_name(api::MY_SERVER_NAME).expect("can't register server");
    trace!("registered with NS -- {:?}", sid);

    // only needed if doing callbacks
    let mut cb_conns: [Option<CID>; 32] = [None; 32];

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
          let rms = buffer.to_flat::<RichMemStruct, _>().unwrap();
          // we can now access fields in rms without copying the data
          // use to_original if you need to invoke methods on the struct object
        }
        Some(Opcode::ExampleMemoryWithReturn) => {
          let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
          let rms = buffer.to_original::<RichMemStruct, _>().unwrap();
          let response = api::Return;
          if things_look_okay {
            let retstruct = AnotherRichStruct {
              name: String::<64>::from_str(rms.as_str()),
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
            let cid = Some(xous::connect(sid).unwrap());
            let mut found = false;
            for entry in cb_conns.iter_mut() {
                if *entry == None {
                    *entry = cid;
                    found = true;
                    break;
                }
            }
            if !found {
                error!("RegisterCallback listener ran out of space registering callback");
            }
          }
        ),
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
fn do_callback(&mut cb_conns: [Option<CID>; 32]) {
  let a = useful_computation();
  for maybe_conn in cb_conns.iter_mut() { // this code is notional and probably doesn't work
    if let Some(conn) = maybe_conn {
       match xous::send_message(conn,
         xous:Message::new_scalar(api::Callback::Hello.to_usize().unwrap(), a, 0, 0, 0)
       ) {
         Err(xous::Error::ServerNotFound) => {
           maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
         },
         Ok => {}
         _ => panic!("unhandled error in callback processing");
       }
    }
  }
}
```
