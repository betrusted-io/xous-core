# mtxchat

This is the skeleton of a Signal chat application.

The UI and local storage are provided by the xous chat library.

Contributions to development of apps/sigchat and libs/chat are most welcome.


## Build

Run sigchat in hosted mode with ```cargo xtask run --app sigchat ```

Build sigchat for the Precursor device with ``` cargo xtask app-image --app sigchat```


## Prerequisites:


## Functionality

sigchat provides the following basic functionality:
* 


## Structure

The `sigchat` code is primarily concerned with the Signal specific protocols, while the Chat library handles the UI and pddb storage.

The Chat library provides the UI to display a series of Signal post (Posts) in a Signal group (Dialogue) stored in the pddb. Each Dialogue is stored in the `pddb:dict` `sigchat.dialogue` under a descriptive `pddb:key` (ie ``).

`sigchat` passes a menu to the Chat UI:

The `sigchat` servers is set to receive:
* `SigchatOp::Post` A memory msg containing an outbount user post
* `SigchatOp::Event` A scalar msg containing important Chat UI events
* `sigchat::Menu` A scalar msg containing click on a sigchat MenuItem
* `SigchatOp::Rawkeys` A scalar msg for each keystroke  


## Troubleshooting

