# Release 0.8 notes

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
