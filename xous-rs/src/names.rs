#![allow(dead_code)]

/*
A list of descriptions of servers ("names") that are used as keys by the xous-names server
to refer to servers that are not statically bound into the kernel, or have a special debug purpose.
*/

pub const SERVER_NAME_IME_PLUGIN_SHELL: &str = "_IME shell plugin_";

/*

Global message IDs

The top 8 bits of message ID fields are reserved for Xous.
Thus IDs are structured as follows:

global ID    message ID
      |       |
    0xGG_MMMMMMM

Here is the allocation so far:

0x00   - private use by individual servers, allocated in api.rs for each server
0x01   - keyboard responses
0x02   - COM responses

*/

pub const GID_KEYBOARD_KEYSTATE_EVENT: usize = 0x01_000000;
pub const GID_KEYBOARD_RAW_KEYSTATE_EVENT: usize = 0x01_000001;

pub const GID_COM_BATTSTATS_EVENT: usize = 0x02_000000;
