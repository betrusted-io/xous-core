# Xous Flattened IPC

Zero-copy IPC for Xous via clever type annotations.

## Synopsis

Any type that is `Ipc` may be sent between processes. All members of the type must be `IpcSafe`. Arbitrarily complex IPC types may be constructed so long as all types are `IpcSafe`.

## Simple Example

```rust
use flatipc_derive::Ipc;

#[derive(Ipc)]
#[repr(C)]
pub struct SimpleValue {
    inner: u32,
}

// Create an ordinary value.
let value = SimpleValue { inner: 42 };

// Turn the value into an IPC-capable structure. This destroys the
// original value. `ipc_value` is page-aligned and padded to a
// multiple of the page size.
let ipc_value = value.to_ipc();

// Even though it's a different type, we can still treat it as if
// it were the original type.
assert_eq!(ipc_value.inner, 42);

// We can also convert it back to the original type.
let value = SimpleValue::from_ipc(ipc_value);
```

## Using IPC

An important feature is the ability to send the data across process boundaries. Two common operations
in Xous are `lend` and `lend_mut()`. This detaches the data from the current process and attaches it
to the target process. The target process can then use the data as if it were its own, and will return
the data and unblock the sender when it returns the message.

Additionally, it is required that the type be `#[repr(C)]`. This ensures that it has a well-defined layout.

```rust
#[derive(flatipc_derive::Ipc)]
#[repr(C)]
pub struct SimpleValue {
    inner: u32,
}

// Immediately create an IPC-capable value.
let ipc_value = SimpleValue { inner: 42 }.to_ipc();

// Lend the IPC value to a server. Note that we need to have previously attached
// to the server via `connection`.
let opcode = 0x1234; // Arbitrary opcode.
ipc_value.lend_mut(connection, opcode).unwrap();
// Execution resumes here after the server returns the value. The value
// was incremented by the server.
assert_eq!(ipc_value.inner, 43);
```

Within the server, we can receive the IPC value and use it as if it were our own.

```rust
#[derive(flatipc_derive::Ipc)]
#[repr(C)]
pub struct SimpleValue {
    inner: u32,
}

let mut message =   None;
while connection.receive(&mut message).is_ok() {;
    let message = message.unwrap();
    match message.opcode {
        0x1234 => {
            let Some(value) = IpcSimpleValue::from_ipc_mut(&mut message.data, message.signature) else {
                continue;
            }
            println!("The value is {}", value.inner);
            value.inner += 1;
        }
        // ...
    }
}
```

It's possible to send mutable data across process boundaries as well. This is done with `lend_mut()`.
Data mutated in the target process will be reflected in the source process when the value is returned.

## Special Types

All types must be `IpcSafe`. This type is derived for all primitives as well as for more common types
such as `Option<T>`.

You can mark your complex types as `IpcSafe` by implementing the trait for them. If your type is comprised
of entirely primitive types, you can `#[derive(IpcSafe)]` on your type.

Because `String` and `Vec` require pointers under the hood, they are not IPC safe. Instead, custom
`String` and `Vec` types are provided that require the user to specify the maximum length of the string.
This enables the receiver to write into the string and have the result reflected in the caller without
needing to allocate more memory for very long strings.

## Traits on the Original Type

IPC types can be turned back into the Original type with `Deref` and `DerefMut`. This allows you to
use the IPC type as if it were the original type by adding `*`. For example:

```rust
#[derive(flatipc_derive::Ipc, PartialEq, Debug)]
#[repr(C)]
struct Value(u32);
let x = Value(42).into_ipc();
let y = Value(42);

// `x` is an `IpcValue`, `y` is a `Value`. By dereferencing `x`, we can
// compare it to `y` using the `PartialEq` trait.
assert_eq!(*x, y);
```
