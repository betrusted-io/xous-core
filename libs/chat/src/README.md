# chat lib

This Chat library provides a generic Chat UI to read a series of Posts in a Dialogue stored in the pddb - and to Author a new Post.

The idea is to provide a solid common Xous Chat interface, so that Chat Apps can concentrate on the unique protocols of each platform (such as Matrix or Signal).

This is a work-in-progress accepting all contributions from ideas and discussion, thru to pull-requests.


## UI User Interface

A Dialogue of Posts appears as a cascade of "speech bubbles" with the most recent at the bottom. The user can use the up/down ↑↓ keys to scroll up/down thru the Posts. One Post will be hilited/selected and the right → key will provide options to reply, edit, delete etc. The left ← key will show the Chat App menu to login, logout, change conversation etc.

A new Post may be authored in the input area at the bottom of the screen.

The Chat UI can provide read-only access to Dialogues in the pddb when offline.


## Integration

A Chat App will typically call `Chat::new()` to setup a Chat UI server, and a 2 way connection between the Chat App server and the Chat UI server. The Chat UI can raise a menu on behalf of the Chat App, allowing for protocol specific actions. The Chat App can receive messages representing:
* a new outbound user Post
* key UI events, such as F1 click, Left click, Top Post, etc,
* raw keystrokes.

The Chat App will next typically call `Chat::dialogue_set()` with a pddb dict and key holding a Dialogue of Posts. 

When the Chat App receives a new Post from the Platform, it will call Chat::post_add() to have it saved in the pddb, and displayed.