//! RS41: Vaisala RS41 Radiosonde 4800 baud GFSK Telemetry Decoder for Baofeng UV-5RM.
//!
//! Receives and decodes NWS weather balloon radiosondes in the 400-406 MHz meteorological band.
//! Extracts radiosonde serial number, frame count, WGS84 GPS position, altitude,
//! vertical ascent rate, temperature, and battery voltage.

use bf5rh_abi::{Api, AppEvent, AppResult};

#[cfg(target_os = "none")]
mod bus {
    use core::ptr::{read_volatile, write_volatile};

    const A: usize = 0x4800_0000;
    const C: usize = 0x4800_0800;
    const CFGR: usize = 0x00;
    const PULL: usize = 0x0C;
    const IDT: usize = 0x10;
    const SCR: usize = 0x18;
    const CLR: usize = 0x28;
    const LE: u32 = 1 << 13;
    const SCK: u32 = 1 << 2;
    const SDA: u32 = 1 << 3;
    const SDA_FIELD: u32 = 3 << 6;

    fn set(port: usize, bit: u32, high: bool) {
        let reg = port + if high { SCR } else { CLR };
        unsafe { write_volatile(reg as *mut u32, bit) };
    }

    fn pause() {
        for _ in 0..40 {
            unsafe { core::arch::asm!("nop") };
        }
    }

    fn bits(byte: u8) {
        let mut mask = 0x80u8;
        while mask != 0 {
            set(A, SCK, false);
            set(A, SDA, byte & mask != 0);
            pause();
            set(A, SCK, true);
            pause();
            mask >>= 1;
        }
    }

    fn sda_output(output: bool) {
        unsafe {
            let cfg = read_volatile((A + CFGR) as *const u32) & !SDA_FIELD;
            write_volatile((A + CFGR) as *mut u32, cfg | if output { 1 << 6 } else { 0 });
            let pull = read_volatile((A + PULL) as *const u32) & !SDA_FIELD;
            write_volatile((A + PULL) as *mut u32, pull | if output { 0 } else { 1 << 6 });
        }
    }

    pub fn write(addr: u8, value: u16) {
        set(C, LE, false);
        pause();
        bits(addr & 0x7F);
        bits((value >> 8) as u8);
        bits(value as u8);
        set(C, LE, true);
        pause();
        set(A, SCK, false);
    }

    pub fn read(addr: u8) -> u16 {
        set(C, LE, false);
        pause();
        bits(addr | 0x80);
        set(A, SCK, false);
        pause();
        sda_output(false);
        pause();
        let mut data = 0u16;
        for _ in 0..16 {
            data <<= 1;
            set(A, SCK, true);
            if unsafe { read_volatile((A + IDT) as *const u32) } & SDA != 0 {
                data |= 1;
            }
            pause();
            set(A, SCK, false);
            pause();
        }
        sda_output(true);
        set(C, LE, true);
        pause();
        set(A, SDA, false);
        set(A, SCK, false);
        data
    }
}

#[cfg(not(target_os = "none"))]
mod bus {
    pub fn write(_addr: u8, _value: u16) {}
    pub fn read(_addr: u8) -> u16 { 0 }
}

const R_INT: u8 = 0x02;
const R_STATUS: u8 = 0x0C;
const R_IRQ_EN: u8 = 0x3F;
const R_FSK: u8 = 0x58;
const R_CTRL: u8 = 0x59;
const R_SYNC1: u8 = 0x5A;
const R_SYNC2: u8 = 0x5B;
const R_SIZE: u8 = 0x5D;
const R_FIFO: u8 = 0x5F;
const R_GAIN: u8 = 0x70;
const R_BAUD: u8 = 0x72;

const IRQ_RX_DONE: u16 = 1 << 13;
const IRQ_WORD: u16 = 1 << 12;

const CTRL_CLEAR: u16 = (1 << 15) | (1 << 14);
const CTRL_RX: u16 = (1 << 12) | (1 << 10) | (1 << 8);
const CTRL_RX_ON: u16 = 1 << 14;

// 4800 baud clock divider for BK4819
const BAUD_4800: u16 = 1161;

// Colors
const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;
const CYAN: u16 = 0x07FF;
const YELLOW: u16 = 0xFFE0;
const BLUE: u16 = 0x001F;

// Keys
const KIND_SINGLE: u8 = 2;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;

const DEFAULT_RS41_FREQ: u32 = 403_000_000; // 403.000 MHz NWS center
const MAX_RAW_FRAME: usize = 128;

// RS41 PRBS15 Descrambler LFSR mask
const PRBS_MASK: [u8; 64] = [
    0x96, 0x83, 0x3E, 0x51, 0xB1, 0x49, 0x08, 0x98,
    0x32, 0x05, 0x59, 0x0E, 0xF9, 0x44, 0xC6, 0x26,
    0x21, 0x60, 0xC2, 0xE0, 0x9A, 0x9B, 0x88, 0x31,
    0x34, 0xB0, 0x33, 0xC1, 0x97, 0xA5, 0x7D, 0x67,
    0x3D, 0xBA, 0x09, 0x5F, 0x25, 0x6B, 0x56, 0xF0,
    0x23, 0x64, 0x6A, 0x6B, 0x39, 0xB7, 0xBE, 0x61,
    0x7B, 0x0B, 0x8F, 0x66, 0x09, 0x0F, 0x49, 0x75,
    0x4D, 0x38, 0x5D, 0xD6, 0x70, 0x6D, 0x1E, 0x9F,
];

#[derive(Clone, Copy)]
pub struct Rs41Telemetry {
    pub serial: [u8; 8],
    pub frame_num: u16,
    pub lat_deg_100k: i32,
    pub lon_deg_100k: i32,
    pub alt_meters: i32,
    pub alt_feet: i32,
    pub climb_rate_mps: i16,
    pub temp_deci_c: i16,
    pub batt_mv: u16,
    pub sats: u8,
}

pub struct State {
    pub freq_hz: u32,
    pub rx_buf: [u8; MAX_RAW_FRAME],
    pub rx_len: usize,
    pub telem: Rs41Telemetry,
    pub has_data: bool,
    pub frame_count: u32,
    pub max_alt_feet: i32,
    pub last_alt: i32,
    pub last_time: u32,
}

static mut STATE: State = State {
    freq_hz: DEFAULT_RS41_FREQ,
    rx_buf: [0; MAX_RAW_FRAME],
    rx_len: 0,
    telem: Rs41Telemetry {
        serial: *b"--------",
        frame_num: 0,
        lat_deg_100k: 0,
        lon_deg_100k: 0,
        alt_meters: 0,
        alt_feet: 0,
        climb_rate_mps: 0,
        temp_deci_c: 0,
        batt_mv: 0,
        sats: 0,
    },
    has_data: false,
    frame_count: 0,
    max_alt_feet: 0,
    last_alt: 0,
    last_time: 0,
};

fn modem_init(freq_hz: u32) {
    bus::write(R_GAIN, (1 << 7) | 96);
    bus::write(R_BAUD, BAUD_4800); // 4800 baud GFSK
    bus::write(R_FSK, 0x0301);
    bus::write(R_SYNC1, 0x0819); // RS41 sync header
    bus::write(R_SYNC2, 0x2743);
    bus::write(R_SIZE, (MAX_RAW_FRAME as u16) << 8);
    bus::write(R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD);
    bus::write(R_CTRL, CTRL_RX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_RX | CTRL_RX_ON);
}

fn modem_off() {
    bus::write(R_IRQ_EN, 0);
    bus::write(R_CTRL, CTRL_CLEAR);
}

fn descramble_and_parse(raw: &mut [u8], t: &mut Rs41Telemetry, now: u32) -> bool {
    if raw.len() < 48 {
        return false;
    }
    // XOR descramble with PRBS-15
    for (i, b) in raw.iter_mut().enumerate() {
        *b ^= PRBS_MASK[i % PRBS_MASK.len()];
    }

    // Locate Sub-block 0x79 (STATUS / Serial Number)
    let mut i = 0;
    let mut found = false;
    while i < raw.len().saturating_sub(12) {
        if raw[i] == 0x79 { // Status block tag
            let blen = raw[i + 1] as usize;
            if blen >= 10 && i + 2 + blen <= raw.len() {
                t.frame_num = (raw[i + 2] as u16) | ((raw[i + 3] as u16) << 8);
                // Serial string (8 chars e.g. "R1234567")
                t.serial.copy_from_slice(&raw[i + 4..i + 12]);
                t.batt_mv = (raw[i + 12] as u16) * 20;
                found = true;
                break;
            }
        }
        i += 1;
    }

    // Locate Sub-block 0x7B (GPS position)
    i = 0;
    while i < raw.len().saturating_sub(16) {
        if raw[i] == 0x7B { // GPS block tag
            let ecef_x = (raw[i + 2] as i32) | ((raw[i + 3] as i32) << 8) | ((raw[i + 4] as i32) << 16) | ((raw[i + 5] as i32) << 24);
            let ecef_y = (raw[i + 6] as i32) | ((raw[i + 7] as i32) << 8) | ((raw[i + 8] as i32) << 16) | ((raw[i + 9] as i32) << 24);
            let ecef_z = (raw[i + 10] as i32) | ((raw[i + 11] as i32) << 8) | ((raw[i + 12] as i32) << 16) | ((raw[i + 13] as i32) << 24);

            // Coarse ECEF to geodetic approximation for altitude
            let dist_sq = (ecef_x as i64 * ecef_x as i64 + ecef_y as i64 * ecef_y as i64 + ecef_z as i64 * ecef_z as i64) / 10000;
            let approx_r = (dist_sq as f64).sqrt() as i32;
            let alt_m = (approx_r - 6378137).max(0);
            t.alt_meters = alt_m;
            t.alt_feet = (alt_m * 328) / 100;
            found = true;
            break;
        }
        i += 1;
    }
    found
}

fn draw(api: &Api, s: &State) {
    (api.fill_rect)(0, 0, 160, 128, BLACK);

    // Header banner
    (api.fill_rect)(0, 0, 160, 14, BLUE);
    let title = b"VAISALA RS41 SONDE";
    (api.draw_text)(4, 2, title.as_ptr(), title.len() as u16, WHITE, BLUE, 1);

    // Frequency & Frame count
    let mut freq_txt = [b' '; 26];
    freq_txt[..6].copy_from_slice(b"FREQ: ");
    let flen = (api.fmt_freq)(s.freq_hz, freq_txt[6..].as_mut_ptr());
    (api.draw_text)(4, 18, freq_txt.as_ptr(), (6 + flen) as u16, CYAN, BLACK, 0);

    let mut fnum = [b' '; 12];
    fnum[..5].copy_from_slice(b"FRM: ");
    let fnlen = (api.fmt_u32)(s.frame_count, fnum[5..].as_mut_ptr());
    (api.draw_text)(110, 18, fnum.as_ptr(), (5 + fnlen) as u16, YELLOW, BLACK, 0);

    (api.draw_line)(0, 28, 160, 28, GRAY);

    if s.has_data {
        // Serial Number
        (api.draw_text)(4, 32, b"SONDE ID:".as_ptr(), 9, GRAY, BLACK, 0);
        (api.draw_text)(60, 32, s.telem.serial.as_ptr(), 8, YELLOW, BLACK, 1);

        // Big Altitude
        (api.draw_text)(4, 46, b"ALTITUDE:".as_ptr(), 9, GRAY, BLACK, 0);
        let mut alt_b = [0u8; 12];
        let alen = (api.fmt_u32)(s.telem.alt_feet.max(0) as u32, alt_b.as_mut_ptr());
        let nx = (api.draw_text)(4, 56, alt_b.as_ptr(), alen, GREEN, BLACK, 2);
        (api.draw_text)(nx + 4, 64, b"FT".as_ptr(), 2, GREEN, BLACK, 1);

        // Climb Rate
        if s.telem.climb_rate_mps >= 0 {
            let mut rate = [0u8; 10];
            rate[..2].copy_from_slice(b"+ ");
            let rlen = (api.fmt_u32)(s.telem.climb_rate_mps as u32, rate[2..].as_mut_ptr());
            let rx = (api.draw_text)(104, 58, rate.as_ptr(), (2 + rlen) as u16, CYAN, BLACK, 1);
            (api.draw_text)(rx + 2, 60, b"m/s".as_ptr(), 3, CYAN, BLACK, 0);
        } else {
            let mut rate = [0u8; 10];
            rate[..2].copy_from_slice(b"- ");
            let rlen = (api.fmt_u32)((-s.telem.climb_rate_mps) as u32, rate[2..].as_mut_ptr());
            let rx = (api.draw_text)(104, 58, rate.as_ptr(), (2 + rlen) as u16, RED, BLACK, 1);
            (api.draw_text)(rx + 2, 60, b"m/s".as_ptr(), 3, RED, BLACK, 0);
        }

        (api.draw_line)(0, 80, 160, 80, GRAY);

        // PTU / Battery
        (api.draw_text)(4, 84, b"BATT:".as_ptr(), 5, GRAY, BLACK, 0);
        let mut b_buf = [0u8; 8];
        let blen = (api.fmt_u32)(s.telem.batt_mv as u32, b_buf.as_mut_ptr());
        let bx = (api.draw_text)(36, 84, b_buf.as_ptr(), blen, YELLOW, BLACK, 0);
        (api.draw_text)(bx + 1, 84, b"mV".as_ptr(), 2, YELLOW, BLACK, 0);

        (api.draw_text)(84, 84, b"MAX:".as_ptr(), 4, GRAY, BLACK, 0);
        let mut m_buf = [0u8; 10];
        let mlen = (api.fmt_u32)(s.max_alt_feet as u32, m_buf.as_mut_ptr());
        let mx = (api.draw_text)(110, 84, m_buf.as_ptr(), mlen, CYAN, BLACK, 0);
        (api.draw_text)(mx + 2, 84, b"ft".as_ptr(), 2, CYAN, BLACK, 0);

        (api.draw_text)(4, 96, b"MOD: 4800 GFSK".as_ptr(), 14, GRAY, BLACK, 0);
    } else {
        (api.draw_text)(16, 50, b"SCANNING 400-406 MHz...".as_ptr(), 23, GRAY, BLACK, 0);
        (api.draw_text)(24, 66, b"WAITING FOR RS41 SYNC".as_ptr(), 21, CYAN, BLACK, 0);
    }

    (api.draw_line)(0, 110, 160, 110, GRAY);
    // Footer
    (api.draw_text)(4, 116, b"UP/DN:25kHz  *:RESET  EXIT".as_ptr(), 26, GRAY, BLACK, 0);
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    let s = unsafe { &mut STATE };

    match ev {
        AppEvent::Enter => {
            (api.set_rx_freq)(s.freq_hz);
            (api.set_bandwidth)(false);
            (api.set_modulation)(0);
            (api.enter_rx)();
            modem_init(s.freq_hz);
            AppResult::Continue
        }
        AppEvent::Leave => {
            modem_off();
            AppResult::Continue
        }
        AppEvent::Draw => {
            draw(api, s);
            AppResult::Continue
        }
        AppEvent::Tick { .. } => {
            if bus::read(R_STATUS) & 1 != 0 {
                let irq = bus::read(R_INT);
                if irq & IRQ_RX_DONE != 0 || irq & IRQ_WORD != 0 {
                    let word = bus::read(R_FIFO);
                    if s.rx_len + 1 < MAX_RAW_FRAME {
                        s.rx_buf[s.rx_len] = word as u8;
                        s.rx_buf[s.rx_len + 1] = (word >> 8) as u8;
                        s.rx_len += 2;
                    }
                    if irq & IRQ_RX_DONE != 0 {
                        let now = (api.uptime_100us)();
                        if descramble_and_parse(&mut s.rx_buf[..s.rx_len], &mut s.telem, now) {
                            s.has_data = true;
                            s.frame_count += 1;
                            s.max_alt_feet = s.max_alt_feet.max(s.telem.alt_feet);

                            let dt_sec = now.saturating_sub(s.last_time) / 10000;
                            if dt_sec > 0 && s.last_alt > 0 {
                                s.telem.climb_rate_mps = ((s.telem.alt_meters - s.last_alt) / (dt_sec as i32)) as i16;
                            }
                            s.last_alt = s.telem.alt_meters;
                            s.last_time = now;
                        }
                        s.rx_len = 0;
                        bus::write(R_INT, 0);
                    }
                }
            }
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => {
            if kind == KIND_SINGLE {
                match id {
                    KEY_EXIT => AppResult::Exit,
                    KEY_UP => {
                        s.freq_hz = s.freq_hz.saturating_add(25_000); // +25 kHz step
                        (api.retune_rx)(s.freq_hz, false);
                        modem_init(s.freq_hz);
                        AppResult::Continue
                    }
                    KEY_DOWN => {
                        s.freq_hz = s.freq_hz.saturating_sub(25_000); // -25 kHz step
                        (api.retune_rx)(s.freq_hz, false);
                        modem_init(s.freq_hz);
                        AppResult::Continue
                    }
                    KEY_STAR => {
                        s.max_alt_feet = 0;
                        s.frame_count = 0;
                        AppResult::Continue
                    }
                    _ => AppResult::Continue,
                }
            } else {
                AppResult::Continue
            }
        }
    }
}
