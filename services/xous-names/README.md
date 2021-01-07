# xous-names

Server IDs (SIDs) are used by processes to send messages to
servers. It is thus an attack surface. Furthermore, if a process can
forge an SID before a server can claim it, it can “become” the
server. Thus, it’s helpful to keep the SID a secret.

In Xous, an SID is a 128-bit number that, with two exceptions, is only
known to the server itself, and an oracle known as `xous-name-server`.

SIDs are never revealed to a process. Processes establish connections
to servers using a descriptive, human-readable string name. Since the
SIDs are random numbers, there is no way to turn the descriptive
string into the SID except by resolving it with `xous-name-server`.

On boot, a trusted set of processes are started which form the
operating system. These must all claim names in the name space before
any further processes are started, to prevent later processes from
claiming their names.

The exception to random SIDs are the `xous-name-server`,
`xous-log-server ` (note the trailing space), and `ticktimer-server`.
These are three well known names that have a defined,
fixed SID so that all processes can talk to them; `xous-name-server` is
necessarily well-known as it is the mechanism to resolve further
names, `xous-log-server ` is necessary for debugging, so that bugs upstream
of name resolution can be logged. `xous-name-server` was picked because
`name-server` already has a meaning in the context of DNS. `ticktimer-server`
is necessary for implementing deterministic timing delays in
`xous-name-server` upon failure, and also for other processes to
wait a fixed period of time on initialization while the initial set
of name registrations occur.

A new process that intends to receive messages initializes using the
following procedure:

1. It creates a `Registration` IPC structure by wrapping it in `xous::ipc::Sendable`,
using the `new()` initializer.

2. It copies a preferred ASCII name string, limited to 64 characters,
into the `Registration` record. It then uses the `.lend_mut()` method
on the Sendable Registration to lend the structure to `xous-name-server`.
The sending process will block until `xous-name-server` returns.

3. `xous-name-server` returns the borrowed memory to the server, with the
response field with a code that either affirms the name was
registered, or the registration is denied (because the proposed name
is already taken or otherwise invalid). In the case that the registration
is affirmed, the SID field in `Registration` contains the assigned SID
of calling process.

4. If the registration is denied, the server can attempt to
re-register its SID with a different ASCII name string by repeating
steps 2-3.


A process that would like to send a server a message does so using the following procedure:

1. It creates a `Lookup` IPC structure by wrapping it in a `xous::ipc::Sendable`,
using the `new()` initializer.

2. It populates the Lookup structure with the name of the server it is looking up.

3. It uses `.lend_mut()` method to send `xous-name-server` a “Mutable Borrow”
`MemoryMessage`.

4. `xous-name-server` can respond with one of three results:
  A. affirm the connection, where one of the slots contains the connection ID;
  B. a flat denial of the connection; or
  C. a slot containing a request to authenticate.

Here are the cases worked out:

A. affirming the connection: `xous-name-server` would use
`MessageSender.pid()` to extract the sender’s PID, and call
`ConnectForProcess(PID,SID)` on behalf of the sender. The sender can
then use the CID as the first argument to `send_message()`. This is
the common case, and many servers follow this path, such as those
asking for access to the `ticktimer` or other public services.

B. flat denial: `xous-name-server` simply returns a message saying the
request was denied by setting `success` to `false`. This is also the case
when the request is malformed or incorrect. No information shall be leaked about the
nature of the denial. Denials are also delayed to the nearest 0.1 second interval since boot
to eliminate side channels and to rate limit fuzzing requests. Some
services (such as the key server) are restricted to only a set of
trusted process loaded at boot, and therefore it should not be
discoverable.

C. request to authenticate: `xous-name-server` responds with `success` set
to `false`, but `authenticate_request` to `true`. The `pubkey_id` field
is populated with the ID of an acceptable Ed25519 public key for authentication, and
a 256-bit challenge nonce is provided in the `challenge` field. Authentication
consists of the requesting server proving that it has knowledge of a shared
secret, namely, an Ed25519 private key.

Upon generating the request to authenticate, `xous-name-server` computes
the correct response to the challenge and stores it in a table with
a timestamp.

The sending process must then sign the `challenge` and return an
`Authenticate` message, constructed similarly to the `Lookup`
message but with the `response_to_challenge` field filled out. It must
do this before `AUTHENTICATE_TIMEOUT` milliseconds have passed.
The server, upon receipt of an `Authenticate` message, merely checks
if the `response_to_challenge` matches any response stored in its
table, and if it does, it accepts the process as authenticated. It is
cryptographically unlikely for there to be a collision in the table;
however, this implementation is weak to an attacker potentially
stealing the response and using it. That being said, if an attacker
already has that level of control in the calling process, there
are bigger problems.

The `AUTHENTICATE_TIMEOUT` field is used to give `xous-name-server`
a chance to depopulate the response table over time, so that it
does not "leak" memory.
