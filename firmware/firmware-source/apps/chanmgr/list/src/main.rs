#![no_std]
#![no_main]

// CHAN MGR segment 0: the channel list. Segment 1 (../edit) is the channel editor; they hand over through the handoff
// buffer (see common.rs).
#[path = "../../common.rs"]
mod common;
#[path = "../../list.rs"]
mod list;
