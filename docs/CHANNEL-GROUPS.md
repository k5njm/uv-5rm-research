# Channel groups (design note, not built)

Status 2026-11-12: **proposal.** What exists today is described first; nothing below "Proposal" is implemented. The user-facing description of
what exists is the [channel groups section of the user guide](USER-GUIDE.md#channel-groups).

## Today
* One array of 1,000 channel slots, 32 bytes each, at SPI flash 0x0000 (Baofeng's layout; CHIRP, stock and the open firmware share it).
* The core has 16 hardcoded groups (`app/group_table.rs`, `MODES`): a name, a short name and one or two **channel-number ranges**
  (WEATHER = 1-7, REPEATERS = 234-304 and 327-344, ...). Most are from the mod author's channel plan; SIMPLEX (351-380),
  SATELLITES (381-410) and SONDE (411-420) were added here. The index is the group's number: `BAND` then a digit `0`-`9` selects it.
  Selecting one restricts memory mode, typed channel numbers and scans to those numbers. **Each side (A and B) has its own group.**
  The channel number shown is the position in the group (`01` is the first channel of REPEATERS); in ALL it is the radio's number.
  A channel has no group field; its only membership flag is "in scan list".
* On hardware this once read as a fault: with a group other than ALL selected, memory mode said NO CHANNELS although channels
  existed. The menu is titled CH GROUPS and the notice names the group (`NO CH IN WX`), but membership is still by number.

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
Core change (the core has 1,372 bytes of flash free on 2026-11-12, see AGENTS.md; the old stable-toolchain core had 88 bytes), CHAN MGR change, tool support,
a migration step, and hardware verification. Not started.
