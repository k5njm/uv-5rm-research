//! CHAN MGR as the simulator sees it: two segments (list, editor) sharing `common.rs`, exactly the files the two overlay
//! crates (`apps/chanmgr/list`, `apps/chanmgr/edit`) are built from. The scenario runner follows `Chain` between them.
#[path = "../../firmware-source/apps/chanmgr/common.rs"]
pub mod common;
#[path = "../../firmware-source/apps/chanmgr/list.rs"]
pub mod list;
#[path = "../../firmware-source/apps/chanmgr/edit.rs"]
pub mod edit;
