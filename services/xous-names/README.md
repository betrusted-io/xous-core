# xous-names

Server IDs (SIDs) are used by processes to send messages to
servers. It is thus an attack surface. Furthermore, if a process can
forge an SID before a server can claim it, it can “become” the
server. Thus, it’s helpful to keep the SID a secret.

In Xous, an SID is a 128-bit number that, with two exceptions, is only
known to the server itself, and an oracle known as `xous-names`.

SIDs are never revealed to a process. Processes establish connections
to servers using a descriptive, human-readable string name. Since the
SIDs are random numbers, there is no way to turn the descriptive
string into the SID except by resolving it with `xous-names`.

On boot, a trusted set of processes are started which form the
operating system. These must all claim names in the name space before
any further processes are started, to prevent later processes from
claiming their names.

The exception to random SIDs are the `xous-names` and
`log-server`. These are two well known names that have a defined,
fixed SID so that all processes can talk to them; `xous-names` is
necessarily well-known as it is the mechanism to resolve further
names, `log-server` is necessary for debugging, so that bugs upstream
of name resolution can be logged. `xous-names` was picked because
`name-server` already has a meaning in the context of DNS.

A new process that intends to receive messages initializes using the
following procedure:

1. It creates a SID by calling `xous::create_server()` (*note*: this
needs to be refactored to take no argument). The SID is a 128-bit
GUID.

2. It registers the SID by sending `xous-names` a “Mutable Borrow”
`MemoryMessage` consisting of a `struct` containing a preferred UTF-8
name string (limited to how long?) plus its 128-bit SID, as well as an
empty slot for the response. The sending process will block until
`xous-names` returns.

3. `xous-names` returns the borrowed memory to the server, with the
response field with a code that either affirms the name was
registered, or the registration is denied (because the proposed name
is already taken or otherwise invalid).

4. If the registration is denied, the server can attempt to
re-register its SID with a different UTF-8 name string by repeating
steps 2-3.


A process that would like to send a server a message does so using the following procedure:

1. It sends `xous-names` a “Mutable Borrow” `MemoryMessage` consisting
of a `struct` containing the preferred UTF-8 name of the server it
wants to talk to, along with slots for the response codes.

2. `xous-names` can respond with one of three results: A. affirm the
connection, where one of the slots contains the connection ID; B. a
flat denial of the connection; or C. a slot containing a request to
authenticate.

Here are the cases worked out:

A. affirming the connection: `xous-names` would use
`MessageSender.pid()` to extract the sender’s PID, and call
`ConnectForProcess(PID,SID)` on behalf of the sender. The sender can
then use the CID as the first argument to `send_message()`. This is
the common case, and many servers follow this path, such as those
asking for access to the `ticktimer` or other public services.

B. flat denial: `xous-names` simply returns a message saying the
request was denied. This is also the case when the request is
malformed or incorrect. No information shall be leaked about the
nature of the denial. Denials are also delayed by exactly 0.5 seconds
to eliminate side channels and to rate limit fuzzing requests. Some
services (such as the key server) are restricted to only a set of
trusted process loaded at boot, and therefore it should not be
discoverable.

C. request to authenticate: `xous-names` responds with a 32-bit nonce
that the sending process must hash then sign with a private key to
prove that it is authorized to communicate with the requested
server. This is used to handle the intermediate case of a user-crafted
process requiring elevated access.
