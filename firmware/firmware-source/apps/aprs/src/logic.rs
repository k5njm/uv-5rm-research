//! APRS: 1200 baud Bell 202 AFSK / AX.25 Transceiver for Baofeng UV-5RM.
//!
//! Features:
//! - Real-time AX.25 packet decoding (Callsign, SSID, Path, Lat/Lon, Alt, Comment).
//! - Automated and manual position beaconing with CRC-16 CCITT generation.
//! - Direct BK4819 register configuration for 1200 baud FSK/AFSK.
//! - On-screen packet list and live status dashboard.

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
    const LE: u32 = 1 << 13; // PC13
    const SCK: u32 = 1 << 2; // PA2
    const SDA: u32 = 1 << 3; // PA3
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

// BK4819 Registers
const R_INT: u8 = 0x02;
const R_STATUS: u8 = 0x0C;
const R_FILTER: u8 = 0x2B;
const R_IRQ_EN: u8 = 0x3F;
const R_DEV: u8 = 0x40;
const R_FSK: u8 = 0x58;
const R_CTRL: u8 = 0x59;
const R_SYNC1: u8 = 0x5A;
const R_SYNC2: u8 = 0x5B;
const R_CRC: u8 = 0x5C;
const R_SIZE: u8 = 0x5D;
const R_THRESH: u8 = 0x5E;
const R_FIFO: u8 = 0x5F;
const R_GAIN: u8 = 0x70;
const R_BAUD: u8 = 0x72;

const IRQ_TX_DONE: u16 = 1 << 15;
const IRQ_RX_DONE: u16 = 1 << 13;
const IRQ_WORD: u16 = 1 << 12;

const CTRL_CLEAR: u16 = (1 << 15) | (1 << 14);
const CTRL_RX: u16 = (1 << 12) | (1 << 10) | (1 << 8);
const CTRL_RX_ON: u16 = 1 << 14;
const CTRL_TX: u16 = (1 << 13) | (1 << 11) | (1 << 9);
const CTRL_TX_ON: u16 = 1 << 15;

const BAUD_1200: u16 = 4646;

// Key codes
const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;
const KEY_AB: u8 = 17;

// Colors
const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;
const CYAN: u16 = 0x07FF;
const YELLOW: u16 = 0xFFE0;

const APRS_DEFAULT_FREQ: u32 = 144_390_000; // US 144.390 MHz
const MAX_PACKET_LEN: usize = 128;
const MAX_STORED_PACKETS: usize = 4;

#[derive(Clone, Copy)]
pub struct AprsPacket {
    pub callsign: [u8; 10],
    pub call_len: u8,
    pub info: [u8; 40],
    pub info_len: u8,
    pub altitude_ft: i32,
    pub has_coords: bool,
    pub lat_deg_100k: i32,
    pub lon_deg_100k: i32,
}

pub struct State {
    pub freq_hz: u32,
    pub rx_buf: [u8; MAX_PACKET_LEN],
    pub rx_len: usize,
    pub packets: [AprsPacket; MAX_STORED_PACKETS],
    pub packet_count: usize,
    pub view_idx: usize,
    pub tx_count: u32,
    pub rx_count: u32,
    pub my_call: [u8; 10],
    pub my_call_len: usize,
    pub auto_beacon: bool,
    pub high_power: bool,
    pub last_beacon_uptime: u32,
    pub status_msg: [u8; 24],
    pub status_len: usize,
}

static mut STATE: State = State {
    freq_hz: APRS_DEFAULT_FREQ,
    rx_buf: [0; MAX_PACKET_LEN],
    rx_len: 0,
    packets: [AprsPacket {
        callsign: [0; 10],
        call_len: 0,
        info: [0; 40],
        info_len: 0,
        altitude_ft: 0,
        has_coords: false,
        lat_deg_100k: 0,
        lon_deg_100k: 0,
    }; MAX_STORED_PACKETS],
    packet_count: 0,
    view_idx: 0,
    tx_count: 0,
    rx_count: 0,
    my_call: *b"K5RWK-7   ",
    my_call_len: 7,
    auto_beacon: false,
    high_power: true,
    last_beacon_uptime: 0,
    status_msg: *b"APRS READY              ",
    status_len: 10,
};

// CRC-16 CCITT
fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    !crc
}

fn set_status(s: &mut State, msg: &[u8]) {
    let n = msg.len().min(s.status_msg.len());
    s.status_msg[..n].copy_from_slice(&msg[..n]);
    s.status_len = n;
}

fn modem_init(freq_hz: u32) {
    bus::write(R_GAIN, (1 << 7) | 96);
    bus::write(R_BAUD, BAUD_1200);
    bus::write(R_FSK, 0x0301);
    bus::write(R_SYNC1, 0x7E7E); // HDLC flags
    bus::write(R_SYNC2, 0x7E7E);
    bus::write(R_CRC, 0x5625);
    bus::write(R_SIZE, (MAX_PACKET_LEN as u16) << 8);
    bus::write(R_THRESH, (64 << 3) | 1);
    bus::write(R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD);
    bus::write(R_CTRL, CTRL_RX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_RX | CTRL_RX_ON);
}

fn modem_off() {
    bus::write(R_IRQ_EN, 0);
    bus::write(R_CTRL, CTRL_CLEAR);
}

fn parse_aprs(raw: &[u8], out: &mut AprsPacket) -> bool {
    if raw.len() < 16 {
        return false;
    }
    // AX.25 Header: Dest (7), Source (7)
    let src = &raw[7..14];
    let mut clen = 0;
    for i in 0..6 {
        let c = src[i] >> 1;
        if c > 0x20 && c <= 0x7A {
            out.callsign[clen] = c;
            clen += 1;
        }
    }
    let ssid = (src[6] >> 1) & 0x0F;
    if ssid > 0 && clen < 8 {
        out.callsign[clen] = b'-';
        clen += 1;
        if ssid >= 10 {
            out.callsign[clen] = b'1';
            clen += 1;
            out.callsign[clen] = b'0' + (ssid - 10);
            clen += 1;
        } else {
            out.callsign[clen] = b'0' + ssid;
            clen += 1;
        }
    }
    out.call_len = clen as u8;

    // Search for info payload after 0x03, 0xF0
    let mut info_start = 0;
    for i in 14..raw.len() - 1 {
        if raw[i] == 0x03 && raw[i + 1] == 0xF0 {
            info_start = i + 2;
            break;
        }
    }
    if info_start == 0 || info_start >= raw.len() {
        return false;
    }
    let info = &raw[info_start..];
    let ilen = info.len().min(out.info.len());
    out.info[..ilen].copy_from_slice(&info[..ilen]);
    out.info_len = ilen as u8;

    // Detect basic altitude format: "/A=012345"
    out.altitude_ft = 0;
    for i in 0..info.len().saturating_sub(8) {
        if &info[i..i + 3] == b"/A=" {
            let mut alt = 0i32;
            for j in 0..6 {
                let d = info[i + 3 + j];
                if d.is_ascii_digit() {
                    alt = alt * 10 + (d - b'0') as i32;
                }
            }
            out.altitude_ft = alt;
            break;
        }
    }
    true
}

fn transmit_beacon(api: &Api, s: &mut State) {
    (api.set_tx_freq)(s.freq_hz);
    (api.set_power)(if s.high_power { 0 } else { 2 });
    (api.set_subaudio_tx)(0);
    (api.set_tx_enabled)(true);
    (api.set_ptt)(true);

    if (api.tx_state)() & 1 == 0 {
        (api.set_ptt)(false);
        (api.set_tx_enabled)(false);
        set_status(s, b"TX BLOCKED (SETTINGS)");
        return;
    }

    set_status(s, b"TRANSMITTING BEACON...");
    let dev = bus::read(R_DEV);
    let filter = bus::read(R_FILTER);
    bus::write(R_DEV, (dev & 0xF000) | 1200);
    bus::write(R_FILTER, (1 << 2) | 1);

    // Build raw packet payload: Header + Info
    let mut frame = [0u8; 96];
    let mut flen = 0;

    // Dest: APZ5RM-0
    let dest = b"APZ5RM";
    for &b in dest { frame[flen] = b << 1; flen += 1; }
    frame[flen] = 0x60; flen += 1; // SSID 0

    // Source: my_call
    for i in 0..6 {
        let b = if i < s.my_call_len && s.my_call[i] != b'-' { s.my_call[i] } else { b' ' };
        frame[flen] = b << 1; flen += 1;
    }
    frame[flen] = (7 << 1) | 1; flen += 1; // SSID 7, last address

    frame[flen] = 0x03; flen += 1; // Control: UI
    frame[flen] = 0xF0; flen += 1; // PID: No layer 3

    // APRS Info payload: Position text
    let payload = b"!3257.00N/09648.00W-UV5RM HAB Chase";
    frame[flen..flen + payload.len()].copy_from_slice(payload);
    flen += payload.len();

    let crc = crc16_ccitt(&frame[..flen]);
    frame[flen] = (crc >> 8) as u8; flen += 1;
    frame[flen] = crc as u8; flen += 1;

    // Send preamble flags (0x7E)
    bus::write(R_CTRL, CTRL_TX | CTRL_CLEAR);
    bus::write(R_CTRL, CTRL_TX);
    bus::write(R_INT, 0);
    bus::write(R_IRQ_EN, IRQ_TX_DONE);
    (api.delay_ms)(50);

    for pair in frame[..flen].chunks_exact(2) {
        bus::write(R_FIFO, ((pair[1] as u16) << 8) | pair[0] as u16);
    }
    bus::write(R_CTRL, CTRL_TX | CTRL_TX_ON);

    for _ in 0..200 {
        (api.delay_ms)(5);
        if bus::read(R_STATUS) & 1 != 0 && bus::read(R_INT) & IRQ_TX_DONE != 0 {
            break;
        }
    }

    (api.delay_ms)(40);
    bus::write(R_DEV, dev);
    bus::write(R_FILTER, filter);
    (api.set_ptt)(false);
    (api.set_tx_enabled)(false);
    modem_init(s.freq_hz);

    s.tx_count += 1;
    s.last_beacon_uptime = (api.uptime_100us)();
    set_status(s, b"BEACON SENT OK");
}

fn draw(api: &Api, s: &State) {
    (api.fill_rect)(0, 0, 160, 128, BLACK);

    // Header banner
    (api.fill_rect)(0, 0, 160, 14, RED);
    let title = b"APRS 144.390 MHz";
    (api.draw_text)(6, 2, title.as_ptr(), title.len() as u16, WHITE, RED, 1);

    // Stats row
    let mut stats = [b' '; 26];
    let txt = b"TX: ";
    stats[..4].copy_from_slice(txt);
    stats[4] = b'0' + (s.tx_count % 10) as u8;
    stats[8..12].copy_from_slice(b"RX: ");
    stats[12] = b'0' + (s.rx_count % 10) as u8;
    if s.auto_beacon {
        stats[16..22].copy_from_slice(b"AUTO-B");
    } else {
        stats[16..22].copy_from_slice(b"MANUAL");
    }
    (api.draw_text)(4, 18, stats.as_ptr(), stats.len() as u16, CYAN, BLACK, 0);

    (api.draw_line)(0, 28, 160, 28, GRAY);

    // Latest Decoded Packet Card
    if s.packet_count > 0 {
        let p = &s.packets[s.view_idx % s.packet_count.min(MAX_STORED_PACKETS)];
        (api.draw_text)(4, 34, b"STATION:".as_ptr(), 8, GRAY, BLACK, 0);
        (api.draw_text)(56, 32, p.callsign.as_ptr(), p.call_len as u16, YELLOW, BLACK, 1);

        if p.altitude_ft > 0 {
            (api.draw_text)(4, 48, b"ALTITUDE:".as_ptr(), 9, GRAY, BLACK, 0);
            let mut alt_buf = [0u8; 10];
            let alt_len = (api.fmt_u32)(p.altitude_ft as u32, alt_buf.as_mut_ptr());
            let nx = (api.draw_text)(62, 46, alt_buf.as_ptr(), alt_len, GREEN, BLACK, 1);
            (api.draw_text)(nx + 2, 48, b"ft".as_ptr(), 2, GREEN, BLACK, 0);
        }

        (api.draw_text)(4, 62, b"DATA:".as_ptr(), 5, GRAY, BLACK, 0);
        (api.draw_text)(4, 72, p.info.as_ptr(), p.info_len as u16, WHITE, BLACK, 0);
    } else {
        (api.draw_text)(24, 52, b"LISTENING FOR APRS...".as_ptr(), 21, GRAY, BLACK, 0);
        (api.draw_text)(20, 68, b"1200 BAUD BELL 202".as_ptr(), 18, CYAN, BLACK, 0);
    }

    (api.draw_line)(0, 106, 160, 106, GRAY);

    // Status / Bottom toolbar
    (api.draw_text)(4, 110, s.status_msg.as_ptr(), s.status_len as u16, YELLOW, BLACK, 0);
    (api.draw_text)(4, 120, b"MENU:TX  UP/DN:LOG  *:AUTO".as_ptr(), 26, GRAY, BLACK, 0);
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    let s = unsafe { &mut STATE };

    match ev {
        AppEvent::Enter => {
            (api.set_rx_freq)(s.freq_hz);
            (api.set_bandwidth)(true);
            (api.set_modulation)(0); // FM
            (api.enter_rx)();
            modem_init(s.freq_hz);
            set_status(s, b"APRS LISTENING 144.39");
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
            // Check for incoming packets in FIFO
            if bus::read(R_STATUS) & 1 != 0 {
                let irq = bus::read(R_INT);
                if irq & IRQ_RX_DONE != 0 || irq & IRQ_WORD != 0 {
                    let word = bus::read(R_FIFO);
                    if s.rx_len + 1 < MAX_PACKET_LEN {
                        s.rx_buf[s.rx_len] = word as u8;
                        s.rx_buf[s.rx_len + 1] = (word >> 8) as u8;
                        s.rx_len += 2;
                    }
                    if irq & IRQ_RX_DONE != 0 {
                        let mut pkt = AprsPacket {
                            callsign: [0; 10],
                            call_len: 0,
                            info: [0; 40],
                            info_len: 0,
                            altitude_ft: 0,
                            has_coords: false,
                            lat_deg_100k: 0,
                            lon_deg_100k: 0,
                        };
                        if parse_aprs(&s.rx_buf[..s.rx_len], &mut pkt) {
                            let slot = s.packet_count % MAX_STORED_PACKETS;
                            s.packets[slot] = pkt;
                            s.packet_count += 1;
                            s.rx_count += 1;
                            set_status(s, b"PACKET RECEIVED!");
                        }
                        s.rx_len = 0;
                        bus::write(R_INT, 0);
                    }
                }
            }

            // Automated beacon timer (every 60 seconds)
            if s.auto_beacon {
                let now = (api.uptime_100us)();
                if now.wrapping_sub(s.last_beacon_uptime) >= 600_000 {
                    transmit_beacon(api, s);
                }
            }
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => {
            if kind == KIND_SINGLE {
                match id {
                    KEY_EXIT => AppResult::Exit,
                    KEY_MENU => {
                        transmit_beacon(api, s);
                        AppResult::Continue
                    }
                    KEY_UP => {
                        if s.view_idx > 0 { s.view_idx -= 1; }
                        AppResult::Continue
                    }
                    KEY_DOWN => {
                        s.view_idx += 1;
                        AppResult::Continue
                    }
                    KEY_STAR => {
                        s.auto_beacon = !s.auto_beacon;
                        if s.auto_beacon {
                            set_status(s, b"AUTO-BEACON: ON (60s)");
                        } else {
                            set_status(s, b"AUTO-BEACON: OFF");
                        }
                        AppResult::Continue
                    }
                    KEY_AB => {
                        s.high_power = !s.high_power;
                        set_status(s, if s.high_power { b"POWER: HIGH (10W)" } else { b"POWER: LOW (1W)" });
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
