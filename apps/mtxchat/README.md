# mtxchat

This is the skeleton of a [matrix] chat application.

The UI and local storage are provided by the xous chat library.

Contributions to development of apps/mtxchat and libs/chat are most welcome.


## Build

Run mtxchat in hosted mode with ```cargo xtask run --app mtxchat ```

Build mtxchat for the Precursor device with ``` cargo xtask app-image --app mtxchat```


## Prerequisites:

Use another device such as a mobile or PC to:

* Create a new Matrix user (if needed):
  * https://matrix.org/faq/#how-do-i-get-an-account-and-get-started%3F
  * https://matrix.org/docs/projects/try-matrix-now/
* Find (or create) a new room to join for chatting:
  * https://doc.matrix.tu-dresden.de/en/rooms/create/
  * https://spec.matrix.org/latest/#room-structure
* Join the room


## Functionality

mtxchat provides the following basic functionality:
* login to an existing account on a [matrix] server
* nominate an existing room on a [matrix] server
* read recent posts
* post text to the room


## Structure

The `mtxchat` code is primarily concerned with the [matrix] specific protocols, while the Chat library handles the UI and pddb storage.

Much of the code in `main.rs` & `lib.rs` has been adapted from `mtxcli`, while `url.rs` and `web.rs` are simple copies.

The Chat library provides the UI to display a series of matrix events (Posts) in a matrix room (Dialogue) stored in the pddb. Each Dialogue is stored in the `pddb:dict` `mtxchat` under a descriptive `pddb:key` (ie `#xous-apps:matrix.org`).

`mtxchat` passes a menu to the Chat UI:
* `room` to type a [matrix] room/server
* `login` to type a username/server & passwords
* `logout`

The `mtxchat` servers is set to receive:
* `MtxchatOp::Post` A memory msg containing an outbount user post
* `MtxchatOp::Event` A scalar msg containing important Chat UI events
* `mtxchat::Menu` A scalar msg containing click on a mtxchat MenuItem
* `MtxchatOp::Rawkeys` A scalar msg for each keystroke  


## Troubleshooting

If you see the message `WARNING: clock not set` that is likely because the Precursor real time clock needs to be set (e.g. if the battery has been completely discharged). Please go to the menu **Preferences | Set Timezone** to set the time zone (and update the time via NTP).

If you see the message `authentication failed` it might be because the `user` and `password` variables are not set properly.
Alternatively it could be because TLS certificate validation
has failed because the clock has not been set (see above).
