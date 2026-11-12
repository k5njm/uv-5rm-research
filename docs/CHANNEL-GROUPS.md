# Channel groups (design note, not built)

Status 2026-11-11: **proposal.** What exists today is described first; nothing below "Proposal" is implemented.

## Today
* One array of 1,000 channel slots, 32 bytes each, at SPI flash 0x0000 (Baofeng's layout; CHIRP, stock and the open firmware share it).
* The core has 13 hardcoded "modes" (`app/modes.rs`): a name and one or two **channel-number ranges** (WEATHER = 1-7,
  REPEATERS = 234-304 and 327-344, ...), from the mod author's channel plan. Selecting one restricts memory mode and scan to
  those numbers. A channel has no group field; its only membership flag is "in scan list".
* On hardware this read as a fault: with a group other than ALL selected, memory mode said NO CHANNELS although channels existed.
  The menu is now titled CH GROUPS and the notice names the group, but membership is still by number.

## Proposal: a group is an attribute of the memory
* Keep the single array where it is (CHIRP, backups and every tool keep working).
* Add a **tag table**: one or two bytes per channel, a bit per group (8 or 16 groups), in an SPI-flash sector that is free
  on a real radio. Choose the sector against the pre-flash dump of a real radio, not from the source alone: that dump shows
  data in areas no document here describes (around 0x8000-0x9FFF, 0xC000, 0xE000).
* Group names: a small table beside the tags (editable later); first version can keep the fixed names.
* First run fills the tags from today's number ranges, so nothing changes until the user edits.
* CHAN MGR: pick a group and see its channels; the channel editor gains a GROUPS field. A channel may be in several groups.
* Core: `contains(mode, num)` and `first_channel` read the tag instead of comparing numbers (about 1,000 one-byte reads to
  scan a group; fine).
* PC tools: read/write the tag table through the programming port (needs a port command, or reuse raw read plus a new write).

## Open question: what a short press of VFO/MR does
Today it toggles VFO and memory. The owner's suggestion (Kenwood TH-D74 style): step through channel groups.
Candidate: short press cycles VFO -> group A -> group B -> ... -> VFO, skipping empty groups; long press stays assignable
(SETTINGS > KEYS > VFO/MR LONG). Decide before building: whether ALL is in the cycle, and whether the BAND key keeps the list.

## Cost
Core change (needs the nightly-toolchain core: the stable one has 88 bytes free), CHAN MGR change, tool support, a migration
step, and hardware verification. Not started.
