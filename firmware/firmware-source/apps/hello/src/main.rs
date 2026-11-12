#![no_std]
#![no_main]

use bf5rh_abi::{Api, AppEvent, AppResult};

const KEY_EXIT: u8 = 13;
const KIND_SINGLE: u8 = 2;

const WHITE: u16 = 0xFFFF;
const CYAN: u16 = 0x07FF;

static mut API_PTR: *const Api = core::ptr::null();

static mut DRAWN: bool = false;

#[no_mangle]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    unsafe {
        API_PTR = api as *const Api;
    }

    match ev {
        AppEvent::Enter => {
            unsafe {
                DRAWN = false;
            }
            AppResult::Continue
        }
        AppEvent::Draw => {
            if unsafe { !DRAWN } {
                draw_once(api);
                unsafe {
                    DRAWN = true;
                }
            }
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => {
            if id == KEY_EXIT && kind == KIND_SINGLE {
                AppResult::Exit
            } else {
                #[cfg(feature = "fault-test")]
                if kind == KIND_SINGLE {
                    fault_test(api, id);
                }
                AppResult::Continue
            }
        }
        AppEvent::Tick { .. } | AppEvent::Leave => AppResult::Continue,
    }
}

/// Deliberate crashes, only in a `--features fault-test` build (see Cargo.toml). Each should end in the core's fault
/// screen and then a restart; none of them may leave the transmitter keyed.
#[cfg(feature = "fault-test")]
fn fault_test(api: &Api, key: u8) {
    match key {
        // app_fault with a line number: APP FAULT, SLOT n LINE 1234
        1 => (api.app_fault)(1234),
        // a panic in the app (its handler calls app_fault(0)): APP FAULT, SLOT n PANIC
        2 => panic!(),
        // an undefined instruction in app code: a hard fault with the PC inside the arena: APP CRASH, PC 2000 xxxx
        3 => unsafe { core::arch::asm!("udf #0", options(noreturn)) },
        // a jump to memory that does not exist: a hard fault in the core's view: CORE FAULT, PC 6000 0000
        4 => unsafe {
            let f: extern "C" fn() = core::mem::transmute(0x6000_0001usize);
            f()
        },
        // let this app key the transmitter (hold PTT; DUMMY LOAD ONLY), then press 1: the carrier must drop at once
        5 => (api.set_tx_enabled)(true),
        _ => {}
    }
}

fn draw_once(api: &Api) {
    (api.fill_rect)(20, 30, 40, 24, CYAN);
    (api.draw_circle)(100, 60, 18, WHITE, false);
    (api.draw_line)(10, 100, 150, 100, WHITE);

    let text = b"HELLO OVERLAY";
    (api.draw_text)(10, 10, text.as_ptr(), text.len() as u16, WHITE, 0, 1);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        let ptr = API_PTR;
        if !ptr.is_null() {
            ((*ptr).app_fault)(0);
        }
    }
    loop {}
}
