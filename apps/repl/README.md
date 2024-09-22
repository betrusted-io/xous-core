# REPL

A simple demo application that provides a bare-bones REPL framework.
## Copying This Demo to a New App

1. Copy this demo application, and rename the relevant structures in its `Cargo.toml` and `main.rs`.
2. Add it to the Workspace `default-members` and `members` arrays by editing `./Cargo.toml`
3. Edit the `manifest.json` file as documented in its [README](../README.md)
4. Build using `cargo xtask app-image you_new_app` to create a flashable Xous image with your app in it.
## Details

![screenshot](repl_screenshot.png)

`repl` is a baseline demo application for Xous. It uses a chat-client
style interface (stacking bubbles of text, with user input right-aligned
and shell feedback left-aligned) to facilitate interactions.

It implements the basic envisioned structure of a Xous application, namely:

 - Text input comes via the `ime-frontend`
 - Graphical output is rendered via the `gam`

[Push Events and Listeners](https://github.com/betrusted-io/xous-core/wiki/Push-Events-and-Listeners) provides an overview of the flow of
messages between an Xous application and the Xous Services.

The one demo command provisioned in the `repl` app plays a tone via
the `codec` service. This was chosen because it is about one of the most complicated
and comprehensive things you can do in the `repl` environment, as it requires
real-time callbacks to fill the audio buffer with new samples. If you have
specific questions about how the callbacks work, we encourage you to open
an issue in this repo and ask your question. Your questions will help us
direct effort toward building the documentation base.

To make your own command, copy the `audio.rs` template
in the `apps/repl/src/cmds/` directory, or use this very basic `echo`
template:

```Rust
use crate::{ShellCmdApi, CommonEnv};
use String;

#[derive(Debug)]
pub struct Echo {
}

impl<'a> ShellCmdApi<'a> for Echo {
    cmd_api!(echo); // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        Ok(Some(rest))
    }
}
```

The "verb" is the name of your command, and it is the argument to the `cmd_api!()` macro.

`process()` is your "main" function, and `args` is a string which contains the
rest of the line that was typed into the chat, minus the verb used to identify
your command (e.g. the arguments, but not tokenized or parsed in
any way). `env` contains some items in a common environment, for example, connections
to the GAM, LLIO server or other resources required by your implementation.

Once you've added your command to the directory, go to the `cmds.rs` file, and follow
the four-step instructions embedded within the file, starting around line 40.
