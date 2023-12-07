The structure of the Signal interface is modelled on [signal-cli](https://github.com/AsamK/signal-cli) by AsamK and contributors. signal-cli is a command-line interface for libsignal-service-java. It supports registering, verifying, sending and receiving messages. This development provides a welcome road-map of the various common transactions between a Signal client and server - see the signal-cli [Manual Page](https://github.com/AsamK/signal-cli/blob/master/man/signal-cli.1.adoc). This road-map is sketched out in apps/sigchat/src/manager.rs, but will be subject to refinement as sigchat development progresses.

Manager::methods() will likely make calls on the official [libsignal](https://github.com/signalapp/libsignal#overview) library.

> libsignal contains platform-agnostic APIs used by the official Signal clients and servers, exposed as a Java, Swift, or TypeScript library. The underlying implementations are written in Rust.

> This repository is used by the Signal client apps ([Android](https://github.com/signalapp/Signal-Android), [iOS](https://github.com/signalapp/Signal-iOS), and [Desktop](https://github.com/signalapp/Signal-Desktop)) as well as server-side. Use outside of Signal is unsupported.

libsignal is released under the AGPLv3 licence, and therefore not directly compatible with xous released under the MIT license. Therefore, libsignal must be wrapped in a suitable API, and included as a binary in the hardware image.
