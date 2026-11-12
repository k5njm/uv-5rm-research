//! Where every kind of crash ends: transmitter off, a message, a restart. Before this a hard fault, a panic or an
//! unhandled interrupt spun forever in cortex-m-rt's `loop {}` with the transmitter in whatever state it was in, and an
//! app's `app_fault` reset at once without a word (AUDIT 43).
//!
//! Runs from any context, so it owns nothing: it takes the peripherals by `steal`, uses no statics (not one byte of
//! RAM), no heap, and no `core::fmt`. The text is `fault_text.rs` (pure, unit-tested on the host).
//!
//! Order matters: (1) transmitter off, first, before anything that could itself fault or hang; (2) the LCD, which is
//! re-initialised from scratch because the fault may have hit mid-transfer; (3) a wait in which the power key still
//! switches the radio off; (4) a system reset, the same one the CPS programming session ends with.
use at32f421_pac as pac;
use cortex_m::peripheral::SCB;
use cortex_m_rt::{exception, ExceptionFrame};
use embedded_graphics::mono_font::{
    ascii::{FONT_6X10, FONT_9X18},
    MonoTextStyle,
};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;

use crate::board;
use crate::device::display::Display;
use crate::drivers::fd6818b::Fd6818;
use crate::fault_text::{self, Fault, RESTART_MS};
use crate::hal::delay;

unsafe extern "C" {
    static _stack_start: u32;
}

/// Never returns. Safe to call from an exception handler, from the loader, or from an app's `app_fault`.
#[inline(never)]
pub(crate) fn stop(fault: Fault) -> ! {
    cortex_m::interrupt::disable();
    let dp = unsafe { pac::Peripherals::steal() };
    let mut syst = unsafe { cortex_m::Peripherals::steal() }.SYST;

    // 1. Transmitter off. The key is the RF chip's PA register on the shared 3-wire bus (PTT itself is only an input), so
    //    first end whatever transaction the fault interrupted: chip select high, clock low, data line an output.
    board::set_fd6818_scn(&dp.gpioc, true);
    board::set_fd6818_sck(&dp.gpioa, false);
    board::set_shared_sda_output(&dp.gpioa);
    let mut rf = Fd6818::new(&dp.gpioa, &dp.gpioc);
    rf.pa_off(&mut syst); // exactly what `enter_rx` and the boot do first
    rf.idle(&mut syst); // ends a keyed tone or any other TX state
    rf.rf_off(&mut syst); // audio muted, PA off again, TX band switches off
    board::set_tx_led(&dp.gpioa, false);
    board::set_rx_led(&dp.gpioa, false);
    board::set_speaker_switch(&dp.gpiob, false);

    // 2. The message. A fresh LCD init: the fault may have hit with the display mid-command.
    let mut display = Display::new(&dp.gpiob, &dp.spi2);
    display.init(&mut syst);
    let lcd = display.as_draw_target();
    crate::ui::boot::clear(lcd);
    let red = Rgb565::new(31, 8, 4);
    let detail = fault_text::detail(fault);
    Text::new(fault_text::title(fault), Point::new(8, 40), MonoTextStyle::new(&FONT_9X18, red)).draw(lcd).ok();
    Text::new(detail.as_str(), Point::new(8, 62), MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE)).draw(lcd).ok();
    Text::new("RESTARTING", Point::new(8, 90), MonoTextStyle::new(&FONT_6X10, Rgb565::new(13, 26, 13))).draw(lcd).ok();
    board::set_lcd_backlight(&dp.gpioa, true);

    // 3. Wait, and keep the power key working: held for 50 ms it drops the latch, as `check_power_off` does.
    let mut waited = 0;
    while waited < RESTART_MS {
        delay::ms(&mut syst, 25);
        waited += 25;
        if board::read_power_key(&dp.gpiob) {
            delay::ms(&mut syst, 50);
            if board::read_power_key(&dp.gpiob) {
                board::set_power_latch(&dp.gpiof, false);
                loop {
                    cortex_m::asm::wfi();
                }
            }
        }
    }

    // 4. Start over (boot re-initialises the RF chip with a soft reset and the PA held off).
    SCB::sys_reset()
}

/// A hard fault. The stack is the likeliest cause (it grows into the arena), so the report is not made on it: the
/// stacked PC is read first, then the stack pointer is put back at the top of RAM and `stop` runs there.
#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    let pc = ef.pc();
    unsafe {
        core::arch::asm!(
            "msr msp, {top}",
            "bx {entry}",
            top = in(reg) &raw const _stack_start as usize,
            entry = in(reg) core_fault as extern "C" fn(u32) -> ! as usize,
            in("r0") pc,
            options(noreturn),
        )
    }
}

extern "C" fn core_fault(pc: u32) -> ! {
    stop(Fault::Core { pc })
}

/// Anything else that would otherwise spin in cortex-m-rt's default handler: the NMI, an interrupt nobody enabled.
#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    stop(Fault::Irq { n: irqn })
}

/// A panic is a hard fault with a program counter: in the shipped build (`panic=immediate-abort`) the compiler already
/// puts the undefined instruction at the panic site, and this handler, used only by the stable-toolchain build, does the
/// same from here (its PC is this function, so the site is lost there).
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    cortex_m::asm::udf()
}
