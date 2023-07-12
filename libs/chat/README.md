# CHAT

A Chat object provides a generic UI to read a series of Posts in a Dialogue stored in the pddb - and to Author a new Post.

The idea is to provide a solid common Xous Chat interface, so that Chat Apps can specialize in the unique aspects of a chat Platform protocol (Matrix, Signal etc).

A Chat App will typically call `Chat::new()` and then `Chat::dialogue_set()` with a pddb dict and key holding a Dialogue of Posts. The Chat object will fire-up the UI and retrieve the Dialogue from the pddb. The user will be able to peruse the stored Posts in the Dialogue - and potentially Author a new Post. A new User Post will be sent to the Chat App in a `MemoryMessage` via the (optional) `CID` and `Opcode` provided by the Chat App. Conversely, when the Chat App receives a new Post from the Platform, it will call Chat::post_add() to have it saved in the pddb.

The Chat UI will also accept (optional) opcodes to forward raw-keystrokes and UI-Events to the Chat App. This allows the Chat App to respond to key UI Events. For example if the User navigates to the `top` of the list of stored Posts, then the Chat App might retrieve older Posts from the Platform to be stored Dialogue with calls to Chat::post_add().
