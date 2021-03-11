# Shell Chat

`shellchat` is a baseline demo application for Xous. It uses a chat-client
style interface (stacking bubbles of text, with user input right-aligned
and shell feedback left-aligned) to facilitate interactions with the hardware.
It implements the basic envisioned structure of a Xous application, namely:

 - Text input comes via the `ime-frontend`
 - Graphical output is rendered via the `gam`

Other facilities for handling system-level messages, such as menu configurations
and network events, have yet to be implemented. However, they will likely end up
with a demo in this application first.

