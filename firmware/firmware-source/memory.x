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
 * hangs when an app is opened.
 */
SECTIONS
{
  .arena_pin (NOLOAD) :
  {
    . = ABSOLUTE(0x200001D0);
  } > RAM
} INSERT AFTER .bss;

ASSERT(__ebss <= 0x200001D0, "firmware .bss has grown into the overlay arena: shrink it, or move the arena (abi::ARENA_ADDR and every app.x) and rebuild all apps");
ASSERT(__suninit == 0x200001D0, "overlay arena is not at abi::ARENA_ADDR");
