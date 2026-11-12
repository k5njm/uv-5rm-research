//! What the fault screen says and when: pure, no hardware, shared with the host simulator's unit tests
//! (`scripts/dev.sh unit`). The code that stops the radio and draws it is `fault.rs`.
//!
//! No `core::fmt`: text goes through `fmtlite`, like every other screen.
use crate::fmtlite::{Sink, TextBuf};
use bf5rh_abi::{ARENA_ADDR, ARENA_MAX};

/// Wait on the message before the restart, unless the power key is held.
pub(crate) const RESTART_MS: u32 = 2500;

/// What the core is told about a fault. Built in the exception handlers, so it is plain data.
#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum Fault {
    /// An overlay app called `api.app_fault(line)` (every app's panic handler calls it with 0, FSK PROBE with the flash
    /// address it refused to touch). `slot` is the app's launcher slot from 0, `SLOT_NONE` if none was running.
    App { slot: u8, line: u32 },
    /// A hard fault (or a panic, which is one: the shipped build aborts with an undefined instruction) at `pc`.
    Core { pc: u32 },
    /// An interrupt nothing handles, or the NMI.
    Irq { n: i16 },
}

pub(crate) const SLOT_NONE: u8 = 0xFF;

/// Lines this long or longer are a code (an address), not a source line.
const MAX_LINE: u32 = 99_999;

/// A program counter inside the app arena: the fault is in app code, whatever the core's exception said.
pub(crate) fn pc_in_arena(pc: u32) -> bool {
    pc >= ARENA_ADDR as u32 && pc < (ARENA_ADDR + ARENA_MAX) as u32
}

pub(crate) fn title(f: Fault) -> &'static str {
    match f {
        Fault::App { .. } => "APP FAULT",
        Fault::Core { pc } if pc_in_arena(pc) => "APP CRASH",
        Fault::Core { .. } | Fault::Irq { .. } => "CORE FAULT",
    }
}

/// `0800 DA22` (two halves, no padding tricks: `put_hex4` is the one hex helper there is).
fn put_addr(out: &mut TextBuf<26>, v: u32) {
    out.put_hex4(v >> 16);
    out.put_char(' ');
    out.put_hex4(v & 0xFFFF);
}

/// The second line: where. At most 26 characters, the screen's width in the 6x10 font.
pub(crate) fn detail(f: Fault) -> TextBuf<26> {
    let mut t = TextBuf::new();
    match f {
        Fault::App { slot, line } => {
            if slot != SLOT_NONE {
                t.put_str("SLOT ");
                t.put_u32(slot as u32 + 1); // the number the launcher shows
                t.put_char(' ');
            }
            if line == 0 {
                t.put_str("PANIC");
            } else {
                t.put_str(if line > MAX_LINE { "CODE " } else { "LINE " });
                t.put_u32(line);
            }
        }
        Fault::Core { pc } => {
            t.put_str("PC ");
            put_addr(&mut t, pc);
        }
        Fault::Irq { n } => {
            t.put_str("IRQ ");
            t.put_i32(n as i32);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_fault_names_the_slot_as_the_launcher_numbers_it_and_the_line() {
        let f = Fault::App { slot: 7, line: 142 };
        assert_eq!(title(f), "APP FAULT");
        assert_eq!(detail(f).as_str(), "SLOT 8 LINE 142");
        assert_eq!(detail(Fault::App { slot: 0, line: 1 }).as_str(), "SLOT 1 LINE 1");
    }

    #[test]
    fn line_zero_is_an_app_panic_and_a_big_value_is_a_code_not_a_line() {
        assert_eq!(detail(Fault::App { slot: 2, line: 0 }).as_str(), "SLOT 3 PANIC");
        assert_eq!(detail(Fault::App { slot: 7, line: 99_999 }).as_str(), "SLOT 8 LINE 99999");
        // FSK PROBE passes the flash address it refused: 0x1B0000
        assert_eq!(detail(Fault::App { slot: 7, line: 0x1B_0000 }).as_str(), "SLOT 8 CODE 1769472");
        assert_eq!(detail(Fault::App { slot: SLOT_NONE, line: 5 }).as_str(), "LINE 5");
    }

    #[test]
    fn a_core_fault_shows_the_program_counter_in_hex() {
        let f = Fault::Core { pc: 0x0800_DA22 };
        assert_eq!(title(f), "CORE FAULT");
        assert_eq!(detail(f).as_str(), "PC 0800 DA22");
        assert_eq!(detail(Fault::Core { pc: 0 }).as_str(), "PC 0000 0000");
        assert_eq!(detail(Fault::Core { pc: 0xFFFF_FFFF }).as_str(), "PC FFFF FFFF");
    }

    #[test]
    fn a_program_counter_in_the_arena_is_an_app_crash_everywhere_else_a_core_fault() {
        let lo = ARENA_ADDR as u32;
        let hi = (ARENA_ADDR + ARENA_MAX) as u32;
        assert_eq!(title(Fault::Core { pc: lo }), "APP CRASH");
        assert_eq!(title(Fault::Core { pc: hi - 1 }), "APP CRASH");
        assert_eq!(title(Fault::Core { pc: lo - 1 }), "CORE FAULT");
        assert_eq!(title(Fault::Core { pc: hi }), "CORE FAULT");
        assert_eq!(title(Fault::Core { pc: 0x0800_1000 }), "CORE FAULT");
        assert_eq!(detail(Fault::Core { pc: lo + 8 }).as_str(), "PC 2000 02D8");
    }

    #[test]
    fn an_unexpected_interrupt_names_its_number_and_every_line_fits_the_screen() {
        assert_eq!(detail(Fault::Irq { n: 17 }).as_str(), "IRQ 17");
        assert_eq!(detail(Fault::Irq { n: -14 }).as_str(), "IRQ -14"); // the NMI
        assert_eq!(title(Fault::Irq { n: 3 }), "CORE FAULT");
        for f in [
            Fault::App { slot: 254, line: u32::MAX },
            Fault::Core { pc: 0xFFFF_FFFF },
            Fault::Irq { n: i16::MIN },
        ] {
            let d = detail(f);
            assert!(d.as_str().len() <= 26, "{}", d.as_str());
            assert!(!d.as_str().contains('?'), "{}", d.as_str()); // TextBuf turns anything it cannot hold into '?'
        }
    }

    #[test]
    fn the_message_waits_long_enough_to_read_and_not_long_enough_to_look_hung() {
        assert!((1500..=4000).contains(&RESTART_MS));
    }
}
