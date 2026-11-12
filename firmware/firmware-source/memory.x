/* AT32F421C8T7: 64K flash total, split by the stock KDH_Bootloader into
 * a 4K bootloader region (0x08000000-0x08001000, left untouched) and a
 * 60K application region starting at 0x080010000
 */
MEMORY
{
  FLASH : ORIGIN = 0x08001000, LENGTH = 60K
  RAM   : ORIGIN = 0x20000000, LENGTH = 16K
}

/* The overlay arena (the only thing in .uninit) must start exactly where the apps are linked: abi::ARENA_ADDR, the
 * ORIGIN in every app.x. Left to itself it follows .bss and moves whenever the firmware's statics change size.
 * This pads up to the address; if .bss ever grows past it the link fails here instead of producing a core that
 * hangs when an app is opened. __core_statics_end is where .data/.bss really stop (build_core.sh prints the gap).
 */
SECTIONS
{
  .arena_pin (NOLOAD) :
  {
    __core_statics_end = .;
    . = ABSOLUTE(0x200002D0);
  } > RAM
} INSERT AFTER .bss;

/* Headroom: fail while 64 bytes are still left, so growing the core's statics is a deliberate decision (move the arena:
 * abi::ARENA_ADDR, this file, every app.x; rebuild all apps) and never a surprise at the last byte. */
ASSERT(__core_statics_end + 64 <= 0x200002D0, "firmware .data+.bss is within 64 bytes of the overlay arena: shrink the statics, or move the arena up (abi::ARENA_ADDR, memory.x, every app.x) and rebuild all apps");
ASSERT(__ebss <= 0x200002D0, "firmware .bss has grown into the overlay arena: shrink it, or move the arena (abi::ARENA_ADDR and every app.x) and rebuild all apps");
ASSERT(__suninit == 0x200002D0, "overlay arena is not at abi::ARENA_ADDR");
