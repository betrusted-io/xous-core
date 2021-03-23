#![allow(dead_code)]

/*
A list of descriptions of servers ("names") that are used as keys by the xous-names server
for the core set of kernel services.
*/

pub const SERVER_NAME_COM: &str      = "_COM manager_";
pub const SERVER_NAME_LLIO: &str      = "_Low Level I/O manager_";
pub const SERVER_NAME_SHELL: &str    = "_Shell_";
pub const SERVER_NAME_GFX: &str      = "_Graphics_";
pub const SERVER_NAME_KBD: &str      = "_Matrix keyboard driver_";
pub const SERVER_NAME_TRNG: &str     = "_TRNG manager_";
pub const SERVER_NAME_GAM: &str      = "_Graphical Abstraction Manager_";
pub const SERVER_NAME_STATUS: &str   = "_Status bar manager_";
pub const SERVER_NAME_IME_FRONT: &str = "_IME front end_";
pub const SERVER_NAME_IME_PLUGIN_SHELL: &str = "_IME shell plugin_";
pub const SERVER_NAME_SHELLCHAT: &str = "_Shell chat application_";
pub const SERVER_NAME_RTC: &str       = "_Real time clock application server_";

pub const SERVER_NAME_FCCAGENT: &str = "_Agent for EMC Testing_";
pub const SERVER_NAME_BENCHMARK: &str= "_Benchmark target_";

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

pub const GID_KEYBOARD_KEYSTATE_EVENT: usize      = 0x01_000000;
pub const GID_KEYBOARD_RAW_KEYSTATE_EVENT: usize  = 0x01_000001;

pub const GID_COM_BATTSTATS_EVENT: usize          = 0x02_000000;
