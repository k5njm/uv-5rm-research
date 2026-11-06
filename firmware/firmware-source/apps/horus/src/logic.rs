//! HORUS: Project Horus High-Altitude Balloon Telemetry Decoder for Baofeng UV-5RM.
//!
//! Decodes Project Horus Binary v2 telemetry on 434 MHz (or configured HAB frequency).
//! Displays live altitude, vertical velocity, GPS position, satellite count, battery,
//! and temperature directly on the radio's color TFT screen.

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

// Colors
const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;
const CYAN: u16 = 0x07FF;
const YELLOW: u16 = 0xFFE0;
const MAGENTA: u16 = 0xF81F;

// Keys
const KIND_SINGLE: u8 = 2;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;

const DEFAULT_HAB_FREQ: u32 = 434_500_000; // 434.500 MHz
const MAX_FRAME_LEN: usize = 64;

#[derive(Clone, Copy)]
pub struct HorusTelemetry {
    pub payload_id: u16,
    pub counter: u16,
    pub hours: u8,
    pub mins: u8,
    pub secs: u8,
    pub lat_deg_100k: i32,
    pub lon_deg_100k: i32,
    pub alt_meters: i32,
    pub alt_feet: i32,
    pub vert_rate_mps: i16,
    pub sats: u8,
    pub batt_mv: u16,
    pub temp_c: i8,
    pub last_rx_uptime: u32,
}

pub struct State {
    pub freq_hz: u32,
    pub rx_buf: [u8; MAX_FRAME_LEN],
    pub rx_len: usize,
    pub telem: HorusTelemetry,
    pub has_data: bool,
    pub packet_count: u32,
    pub max_alt_feet: i32,
    pub last_alt: i32,
    pub last_alt_time: u32,
}

static mut STATE: State = State {
    freq_hz: DEFAULT_HAB_FREQ,
    rx_buf: [0; MAX_FRAME_LEN],
    rx_len: 0,
    telem: HorusTelemetry {
        payload_id: 0,
        counter: 0,
        hours: 0,
        mins: 0,
        secs: 0,
        lat_deg_100k: 0,
        lon_deg_100k: 0,
        alt_meters: 0,
        alt_feet: 0,
        vert_rate_mps: 0,
        sats: 0,
        batt_mv: 0,
        temp_c: 0,
        last_rx_uptime: 0,
    },
    has_data: false,
    packet_count: 0,
    max_alt_feet: 0,
    last_alt: 0,
    last_alt_time: 0,
};

fn modem_init(freq_hz: u32) {
    bus::write(R_GAIN, (1 << 7) | 96);
    bus::write(R_BAUD, 4646);
    bus::write(R_FSK, 0x0301);
    bus::write(R_SYNC1, 0x2424); // "$$" sync preamble
    bus::write(R_SYNC2, 0x2424);
    bus::write(R_SIZE, (MAX_FRAME_LEN as u16) << 8);
    bus::write(R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD);
    bus::write(R_CTRL, CTRL_RX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_RX | CTRL_RX_ON);
}

fn modem_off() {
    bus::write(R_IRQ_EN, 0);
    bus::write(R_CTRL, CTRL_CLEAR);
}

fn parse_horus(raw: &[u8], t: &mut HorusTelemetry, now: u32) -> bool {
    if raw.len() < 22 {
        return false;
    }
    // Expected Horus binary v2 framing:
    // [0..2] = "$$"
    // [2..4] = Payload ID (u16)
    // [4..6] = Sequence Counter (u16)
    // [6..9] = Time (H, M, S)
    // [9..13] = Latitude (i32 / 1e5)
    // [13..17] = Longitude (i32 / 1e5)
    // [17..20] = Altitude meters (u24)
    // [20] = Speed/Sats
    // [21] = Battery/Temp
    t.payload_id = (raw[2] as u16) << 8 | (raw[3] as u16);
    t.counter = (raw[4] as u16) << 8 | (raw[5] as u16);
    t.hours = raw[6];
    t.mins = raw[7];
    t.secs = raw[8];

    t.lat_deg_100k = ((raw[9] as i32) << 24 | (raw[10] as i32) << 16 | (raw[11] as i32) << 8 | (raw[12] as i32)) / 100;
    t.lon_deg_100k = ((raw[13] as i32) << 24 | (raw[14] as i32) << 16 | (raw[15] as i32) << 8 | (raw[16] as i32)) / 100;

    let alt_m = (raw[17] as i32) << 16 | (raw[18] as i32) << 8 | (raw[19] as i32);
    t.alt_meters = alt_m;
    t.alt_feet = (alt_m * 328) / 100;

    t.sats = raw[20] & 0x1F;
    t.batt_mv = (raw[21] as u16) * 20; // 20 mV units
    if raw.len() >= 23 {
        t.temp_c = raw[22] as i8;
    }
    t.last_rx_uptime = now;
    true
}

fn draw(api: &Api, s: &State) {
    (api.fill_rect)(0, 0, 160, 128, BLACK);

    // Header banner
    (api.fill_rect)(0, 0, 160, 14, MAGENTA);
    let mut hdr = [b' '; 26];
    hdr[..6].copy_from_slice(b"HORUS ");
    let khz = s.freq_hz / 1000;
    let _ = (api.fmt_u32)(khz, hdr[6..].as_mut_ptr());
    (api.draw_text)(4, 2, b"HORUS HAB RECEIVER".as_ptr(), 18, WHITE, MAGENTA, 1);

    // Frequency & Packet Tally
    let mut freq_txt = [b' '; 26];
    freq_txt[..6].copy_from_slice(b"FREQ: ");
    let flen = (api.fmt_freq)(s.freq_hz, freq_txt[6..].as_mut_ptr());
    (api.draw_text)(4, 18, freq_txt.as_ptr(), (6 + flen) as u16, CYAN, BLACK, 0);

    let mut pkts = [b' '; 12];
    pkts[..5].copy_from_slice(b"PKT: ");
    let plen = (api.fmt_u32)(s.packet_count, pkts[5..].as_mut_ptr());
    (api.draw_text)(110, 18, pkts.as_ptr(), (5 + plen) as u16, YELLOW, BLACK, 0);

    (api.draw_line)(0, 28, 160, 28, GRAY);

    if s.has_data {
        // Large Altitude Display
        (api.draw_text)(4, 32, b"ALTITUDE".as_ptr(), 8, GRAY, BLACK, 0);
        let mut alt_str = [0u8; 12];
        let alen = (api.fmt_u32)(s.telem.alt_feet.max(0) as u32, alt_str.as_mut_ptr());
        let nx = (api.draw_text)(4, 42, alt_str.as_ptr(), alen, GREEN, BLACK, 2);
        (api.draw_text)(nx + 4, 50, b"FT".as_ptr(), 2, GREEN, BLACK, 1);

        // Vertical Rate Indicator
        if s.telem.vert_rate_mps >= 0 {
            let mut rate = [0u8; 10];
            rate[..2].copy_from_slice(b"+ ");
            let rlen = (api.fmt_u32)(s.telem.vert_rate_mps as u32, rate[2..].as_mut_ptr());
            let rx = (api.draw_text)(106, 44, rate.as_ptr(), (2 + rlen) as u16, CYAN, BLACK, 1);
            (api.draw_text)(rx + 2, 46, b"m/s".as_ptr(), 3, CYAN, BLACK, 0);
        } else {
            let mut rate = [0u8; 10];
            rate[..2].copy_from_slice(b"- ");
            let rlen = (api.fmt_u32)((-s.telem.vert_rate_mps) as u32, rate[2..].as_mut_ptr());
            let rx = (api.draw_text)(106, 44, rate.as_ptr(), (2 + rlen) as u16, RED, BLACK, 1);
            (api.draw_text)(rx + 2, 46, b"m/s".as_ptr(), 3, RED, BLACK, 0);
        }

        (api.draw_line)(0, 64, 160, 64, GRAY);

        // Coordinates & Telemetry
        (api.draw_text)(4, 68, b"LAT/LON:".as_ptr(), 8, GRAY, BLACK, 0);
        let mut lat_buf = [0u8; 12];
        let lat_l = (api.fmt_u32)(s.telem.lat_deg_100k.abs() as u32, lat_buf.as_mut_ptr());
        (api.draw_text)(56, 68, lat_buf.as_ptr(), lat_l, WHITE, BLACK, 0);

        let mut lon_buf = [0u8; 12];
        let lon_l = (api.fmt_u32)(s.telem.lon_deg_100k.abs() as u32, lon_buf.as_mut_ptr());
        (api.draw_text)(104, 68, lon_buf.as_ptr(), lon_l, WHITE, BLACK, 0);

        // Stats row: Battery, Temp, Satellites
        (api.draw_text)(4, 82, b"TEMP:".as_ptr(), 5, GRAY, BLACK, 0);
        let mut temp_b = [0u8; 8];
        let t_len = (api.fmt_u32)(s.telem.temp_c.abs() as u32, temp_b.as_mut_ptr());
        (api.draw_text)(36, 82, temp_b.as_ptr(), t_len, YELLOW, BLACK, 0);

        (api.draw_text)(68, 82, b"BATT:".as_ptr(), 5, GRAY, BLACK, 0);
        let mut batt_b = [0u8; 8];
        let b_len = (api.fmt_u32)(s.telem.batt_mv as u32, batt_b.as_mut_ptr());
        let bx = (api.draw_text)(98, 82, batt_b.as_ptr(), b_len, YELLOW, BLACK, 0);
        (api.draw_text)(bx + 1, 82, b"mV".as_ptr(), 2, YELLOW, BLACK, 0);

        (api.draw_text)(4, 96, b"MAX ALT:".as_ptr(), 8, GRAY, BLACK, 0);
        let mut max_b = [0u8; 10];
        let mx_len = (api.fmt_u32)(s.max_alt_feet as u32, max_b.as_mut_ptr());
        let mxx = (api.draw_text)(56, 96, max_b.as_ptr(), mx_len, CYAN, BLACK, 0);
        (api.draw_text)(mxx + 2, 96, b"ft".as_ptr(), 2, CYAN, BLACK, 0);
    } else {
        (api.draw_text)(20, 52, b"WAITING FOR HORUS TELEM".as_ptr(), 23, GRAY, BLACK, 0);
        (api.draw_text)(28, 68, b"4-FSK / 2-FSK 434 MHz".as_ptr(), 21, CYAN, BLACK, 0);
    }

    (api.draw_line)(0, 110, 160, 110, GRAY);
    // Footer / Controls
    (api.draw_text)(4, 116, b"UP/DN:TUNE 5kHz  *:RST  EXIT".as_ptr(), 28, GRAY, BLACK, 0);
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    let s = unsafe { &mut STATE };

    match ev {
        AppEvent::Enter => {
            (api.set_rx_freq)(s.freq_hz);
            (api.set_bandwidth)(false); // Narrow FM
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
                    if s.rx_len + 1 < MAX_FRAME_LEN {
                        s.rx_buf[s.rx_len] = word as u8;
                        s.rx_buf[s.rx_len + 1] = (word >> 8) as u8;
                        s.rx_len += 2;
                    }
                    if irq & IRQ_RX_DONE != 0 {
                        let now = (api.uptime_100us)();
                        if parse_horus(&s.rx_buf[..s.rx_len], &mut s.telem, now) {
                            s.has_data = true;
                            s.packet_count += 1;
                            s.max_alt_feet = s.max_alt_feet.max(s.telem.alt_feet);

                            // Calculate vertical speed
                            let dt_sec = now.saturating_sub(s.last_alt_time) / 10000;
                            if dt_sec > 0 && s.last_alt > 0 {
                                s.telem.vert_rate_mps = ((s.telem.alt_meters - s.last_alt) / (dt_sec as i32)) as i16;
                            }
                            s.last_alt = s.telem.alt_meters;
                            s.last_alt_time = now;
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
                        s.freq_hz = s.freq_hz.saturating_add(5_000); // +5 kHz
                        (api.retune_rx)(s.freq_hz, false);
                        modem_init(s.freq_hz);
                        AppResult::Continue
                    }
                    KEY_DOWN => {
                        s.freq_hz = s.freq_hz.saturating_sub(5_000); // -5 kHz
                        (api.retune_rx)(s.freq_hz, false);
                        modem_init(s.freq_hz);
                        AppResult::Continue
                    }
                    KEY_STAR => {
                        s.max_alt_feet = 0;
                        s.packet_count = 0;
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
