#![allow(dead_code)]

/*
This file contains a list of descriptions of servers that
are used as keys by the xous-names server.

It also contains global IDs. By convention, the top 8 bits of
a message ID field are reserved for Xous.
*/

pub const SERVER_NAME_COM: &str      = "_COM manager_";
pub const SERVER_NAME_SHELL: &str    = "_Shell_";
pub const SERVER_NAME_GFX: &str      = "_Graphics_";
pub const SERVER_NAME_FCCAGENT: &str = "_Agent for EMC Testing_";
pub const SERVER_NAME_KBD: &str      = "_Matrix keyboard driver_";

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
