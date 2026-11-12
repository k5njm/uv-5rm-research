//! FSK probe: a hardware bring-up tool for the radio's FSK modem. It answers "does this chip's modem hear anything,
//! and what do its registers say?" before any modem app (APRS, Horus, RS41) is trusted.
//!
//! Receive-only. It never keys the transmitter and never calls `api.tone_on` or `api.set_ptt`. Chip access goes
//! through `api.rf_reg_read` / `api.rf_reg_write`; the core refuses writes outside the modem allow-list
//! (`bf5rh_abi::RF_WRITABLE`), and this app only ever writes registers on that list.
//!
//! Register meanings come from Fudan's "FD6818B Registers Definition" (2025/1/11); see HARDWARE.md. Still UNVERIFIED on
//! the YP07's FD6818B: the crystal (the baud formula depends on it), the sync words, and that the modem hears a real
//! signal at all. Finding that out is the point of this app.

use bf5rh_abi::{Api, AppEvent, AppResult};

const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;
const KEY_POUND: u8 = 11;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const TITLE_BG: u16 = 0x5800;
const GREEN: u16 = 0x07E0;
const YELLOW: u16 = 0xFFE0;
const RED: u16 = 0xF800;

const R_INT: u8 = 0x02;
const R_STATUS: u8 = 0x0C;
const R_IRQ_EN: u8 = 0x3F;
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

const IRQ_SYNC: u16 = 1 << 1;
const IRQ_RX_DONE: u16 = 1 << 13;
const IRQ_WORD: u16 = 1 << 12;

const CTRL_CLEAR: u16 = (1 << 15) | (1 << 14);
const CTRL_RX: u16 = (1 << 12) | (1 << 10) | (1 << 8);
const CTRL_RX_ON: u16 = 1 << 14;

/// Registers the peek view cycles through. Reading is unrestricted; this is just the interesting set.
const PEEK: [u8; 14] = [
    0x02, 0x0C, 0x2B, 0x30, 0x3F, 0x40, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x72,
];

/// Frequencies in Hz: APRS, 2 m calling, 70 cm calling, Horus 70 cm.
/// 404.800 is the Fort Worth NWS radiosonde frequency (the owner's local launch site); the others are test signals.
const FREQS: [u32; 5] = [404_800_000, 144_390_000, 146_520_000, 446_000_000, 434_714_000];
const BAUDS: [u16; 4] = [450, 1200, 2400, 4800];
/// Sync word presets, as the two 16-bit halves written to the chip (keys 5-9). The last two are the first four bytes
/// of the RS41 radiosonde frame header: on the air the bits are 00001000 01101101 01010011 10001000, which is 0x086D
/// 0x5388 if the chip shifts each byte most-significant bit first and 0x10B6 0xCA11 if least-significant first.
/// Which one the chip uses is one of the things this app is for.
const SYNCS: [(u16, u16); 5] = [
    (0x7E7E, 0x7E7E),
    (0x2DD4, 0x2DD4),
    (0xAAAA, 0xAAAA),
    (0x086D, 0x5388),
    (0x10B6, 0xCA11),
];

/// REG_72 (TONE2/FSK frequency control word) = baud * 10.32444 for a 13/26 MHz crystal, * 10.48576 for 12.8/19.2/25.6/38.4 MHz
/// (datasheet). TEXT writes 4646 for 450 baud, which is exactly 450 * 10.32444, so a 13/26 MHz crystal is assumed.
/// Scaled by 1e5 for integer math.
const BAUD_REG_PER_BAUD_1E5: u64 = 1_032_444;

// ---- capture --------------------------------------------------------------------------------------------------
// `#` records what the modem delivers into a fixed range of the radio's data flash, for `probe_capture.py` to
// download. The range is 0x1B0000-0x1D0000: past the app slots and the satellite table, with 64 KB of unused flash
// between its end and the channel extras table at 0x1E0000. Nothing else uses it.
//
//   page 0            header: "FSKCAP1\0", frequency Hz u32, baud u16, sync halves 2 x u16, baud register u16,
//                     start time u32 (100 us units), record size u8; the rest erased
//   pages 1..CAP_PAGES  records, 8 bytes each, little-endian: time u32 (100 us units), FIFO word u16, flags u8
//                     (FLAG_*; bit 7 is always 0, so an erased 0xFF marks the end), RSSI raw u8
//
// It cannot run past the range: every flash write and erase goes through `cap_write` / `cap_erase`, which refuse an
// address outside it, records only ever go to `page_addr`, which has no address for a page past the last, and the
// recorder stops accepting records when the last page is written (the screen then says REC FULL).
pub const CAP_ADDR: u32 = 0x1B_0000;
pub const CAP_BYTES: u32 = 0x2_0000;
const SECTOR: u32 = 4096;
const CAP_SECTORS: u8 = (CAP_BYTES / SECTOR) as u8;
const PAGE: usize = 256;
const REC: usize = 8;
const CAP_PAGES: u16 = (CAP_BYTES / PAGE as u32) as u16;
pub const CAP_RECORDS: u32 = (CAP_PAGES as u32 - 1) * (PAGE / REC) as u32;
const CAP_MAGIC: &[u8; 8] = b"FSKCAP1\0";

const FLAG_SYNC: u8 = 1;
const FLAG_WORD: u8 = 2;
const FLAG_DONE: u8 = 4;

/// FIFO depth: the most words one tick will take before giving the rest of the firmware its turn.
const MAX_WORDS_PER_TICK: u8 = 8;

#[derive(Clone, Copy, PartialEq)]
enum Cap {
    Idle,
    /// Erasing the range, one sector per tick; the value is the next sector.
    Erasing(u8),
    Recording,
    Full,
}

/// Where record page `page` lives, or `None` for the header page and anything past the range.
fn page_addr(page: u16) -> Option<u32> {
    if page >= 1 && page < CAP_PAGES {
        Some(CAP_ADDR + page as u32 * PAGE as u32)
    } else {
        None
    }
}

/// Collects records into one flash page at a time. Pure: the caller supplies the function that writes a page.
struct Recorder {
    page: u16,
    fill: u16,
    stored: u32,
    buf: [u8; PAGE],
}

impl Recorder {
    const NEW: Recorder = Recorder { page: 1, fill: 0, stored: 0, buf: [0xFF; PAGE] };

    fn count(&self) -> u32 {
        self.stored
    }

    /// Adds a record, writing the page out when it fills. False (and nothing stored) once the range is full.
    fn push(&mut self, rec: [u8; REC], write: &mut dyn FnMut(u32, &[u8; PAGE])) -> bool {
        if page_addr(self.page).is_none() {
            return false;
        }
        let at = self.fill as usize;
        self.buf[at..at + REC].copy_from_slice(&rec);
        self.fill += REC as u16;
        self.stored += 1;
        if self.fill as usize == PAGE {
            self.flush(write);
        }
        true
    }

    /// Writes a partly filled page (the unused part stays erased). Does nothing when there is nothing to write.
    fn flush(&mut self, write: &mut dyn FnMut(u32, &[u8; PAGE])) {
        if self.fill == 0 {
            return;
        }
        if let Some(addr) = page_addr(self.page) {
            write(addr, &self.buf);
            self.page += 1;
        }
        self.fill = 0;
        self.buf = [0xFF; PAGE];
    }
}

fn in_capture_range(addr: u32, len: u32) -> bool {
    addr >= CAP_ADDR && len <= CAP_BYTES && addr - CAP_ADDR <= CAP_BYTES - len
}

fn cap_write(api: &Api, addr: u32, data: &[u8]) {
    if !in_capture_range(addr, data.len() as u32) || !(api.nor_write)(addr, data.as_ptr(), data.len() as u32) {
        (api.app_fault)(addr);
    }
}

fn cap_erase(api: &Api, sector: u8) {
    let addr = CAP_ADDR + sector as u32 * SECTOR;
    if sector >= CAP_SECTORS || !in_capture_range(addr, SECTOR) || !(api.nor_erase_sector)(addr) {
        (api.app_fault)(addr);
    }
}

fn header(s: &State, now: u32) -> [u8; PAGE] {
    let mut h = [0xFF; PAGE];
    h[..8].copy_from_slice(CAP_MAGIC);
    h[8..12].copy_from_slice(&FREQS[s.freq].to_le_bytes());
    h[12..14].copy_from_slice(&BAUDS[s.baud].to_le_bytes());
    h[14..16].copy_from_slice(&SYNCS[s.sync].0.to_le_bytes());
    h[16..18].copy_from_slice(&SYNCS[s.sync].1.to_le_bytes());
    h[18..20].copy_from_slice(&baud_reg(BAUDS[s.baud]).to_le_bytes());
    h[20..24].copy_from_slice(&now.to_le_bytes());
    h[24] = REC as u8;
    h
}

fn record(now: u32, word: u16, flags: u8, rssi: u16) -> [u8; REC] {
    let t = now.to_le_bytes();
    let w = word.to_le_bytes();
    [t[0], t[1], t[2], t[3], w[0], w[1], flags & 0x7F, rssi.min(255) as u8]
}

/// `#`: start a capture (erase first), or stop the one that is running.
fn capture_key(api: &Api, s: &mut State) {
    match s.cap {
        Cap::Idle => {
            modem_off(api, s);
            s.rec = Recorder::NEW;
            s.saved = 0;
            s.cap = Cap::Erasing(0);
        }
        Cap::Erasing(_) => {}
        Cap::Recording | Cap::Full => stop_capture(api, s),
    }
}

/// One step of the erase; when the last sector is done, write the header and start the modem.
fn erase_step(api: &Api, s: &mut State, sector: u8) {
    if sector < CAP_SECTORS {
        cap_erase(api, sector);
        s.cap = Cap::Erasing(sector + 1);
    } else {
        cap_write(api, CAP_ADDR, &header(s, (api.uptime_100us)()));
        s.cap = Cap::Recording;
        modem_on(api, s);
    }
    s.dirty = true;
}

/// Ends a capture, writing out the last partly filled page. Safe to call when none is running.
fn stop_capture(api: &Api, s: &mut State) {
    if matches!(s.cap, Cap::Recording | Cap::Full) {
        s.rec.flush(&mut |addr, page| cap_write(api, addr, page));
        s.saved = s.rec.count();
    }
    s.cap = Cap::Idle;
    s.dirty = true;
}

const SCREEN_W: u16 = 160;
const LINE_H: i16 = 10;

struct State {
    freq: usize,
    baud: usize,
    sync: usize,
    running: bool,
    peek: usize,
    syncs: u16,
    words: u16,
    last: [u16; 4],
    status: u16,
    irq: u16,
    dirty: bool,
    rssi: u16,
    cap: Cap,
    saved: u32,
    rec: Recorder,
}

static mut STATE: State = STATE_DEFAULT;

fn st() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    match ev {
        AppEvent::Enter => {
            *st() = STATE_DEFAULT;
            tune(api, st());
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { .. } => {
            tick(api, st());
            AppResult::Continue
        }
        AppEvent::Draw => {
            draw(api, st());
            AppResult::Continue
        }
        AppEvent::Leave => {
            modem_off(api, st());
            AppResult::Continue
        }
    }
}

const STATE_DEFAULT: State = State {
    freq: 0, // radiosonde frequency
    baud: 3, // 4800: the RS41's speed
    sync: 3, // RS41 header, most-significant bit first
    running: false,
    peek: 0,
    syncs: 0,
    words: 0,
    last: [0; 4],
    status: 0,
    irq: 0,
    dirty: true,
    rssi: 0,
    cap: Cap::Idle,
    saved: 0,
    rec: Recorder::NEW,
};

/// Listen on the chosen frequency in narrow FM; the modem taps the demodulator.
fn tune(api: &Api, s: &State) {
    (api.set_modulation)(0);
    (api.set_rx_freq)(FREQS[s.freq]);
    (api.set_subaudio_rx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
}

fn w(api: &Api, addr: u8, v: u16) {
    // A refused write means the allow-list and this app disagree; surface it instead of ignoring it.
    if !(api.rf_reg_write)(addr, v) {
        (api.app_fault)(addr as u32);
    }
}

fn modem_on(api: &Api, s: &mut State) {
    w(api, R_GAIN, (1 << 7) | 96);
    w(api, R_BAUD, baud_reg(BAUDS[s.baud]));
    w(api, R_FSK, 0x0301);
    w(api, R_SYNC1, SYNCS[s.sync].0);
    w(api, R_SYNC2, SYNCS[s.sync].1);
    w(api, R_CRC, 0x5625);
    w(api, R_SIZE, 64u16 << 8);
    w(api, R_THRESH, (1 << 3) | 1);
    w(api, R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD | IRQ_SYNC);
    w(api, R_CTRL, CTRL_RX | CTRL_CLEAR);
    w(api, R_CTRL, CTRL_RX | CTRL_RX_ON);
    s.running = true;
    s.syncs = 0;
    s.words = 0;
    s.last = [0; 4];
}

/// Also ends a capture: nothing records while the modem is off.
fn modem_off(api: &Api, s: &mut State) {
    w(api, R_IRQ_EN, 0);
    w(api, R_CTRL, CTRL_CLEAR);
    s.running = false;
    stop_capture(api, s);
}

fn baud_reg(baud: u16) -> u16 {
    ((baud as u64 * BAUD_REG_PER_BAUD_1E5 + 50_000) / 100_000) as u16
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    let s = st();
    match id {
        KEY_EXIT => {
            modem_off(api, s);
            return AppResult::Exit;
        }
        KEY_POUND => capture_key(api, s),
        // The header describes the whole capture, so the settings stay put until it ends.
        _ if s.cap != Cap::Idle && id != KEY_MENU && id != KEY_UP && id != KEY_DOWN => return AppResult::Continue,
        KEY_MENU => {
            if matches!(s.cap, Cap::Erasing(_)) {
                return AppResult::Continue;
            }
            if s.running {
                modem_off(api, s);
            } else {
                modem_on(api, s);
            }
        }
        KEY_STAR => {
            s.freq = (s.freq + 1) % FREQS.len();
            let was = s.running;
            modem_off(api, s);
            tune(api, s);
            if was {
                modem_on(api, s);
            }
        }
        KEY_UP => s.peek = (s.peek + PEEK.len() - 1) % PEEK.len(),
        KEY_DOWN => s.peek = (s.peek + 1) % PEEK.len(),
        1..=4 => {
            s.baud = (id - 1) as usize;
            if s.running {
                w(api, R_BAUD, baud_reg(BAUDS[s.baud]));
            }
        }
        5..=9 => {
            s.sync = (id - 5) as usize;
            if s.running {
                w(api, R_SYNC1, SYNCS[s.sync].0);
                w(api, R_SYNC2, SYNCS[s.sync].1);
            }
        }
        _ => return AppResult::Continue,
    }
    s.dirty = true;
    AppResult::Continue
}

fn tick(api: &Api, s: &mut State) {
    let status = (api.rf_reg_read)(R_STATUS);
    if status != s.status {
        s.status = status;
        s.dirty = true;
    }
    if let Cap::Erasing(sector) = s.cap {
        erase_step(api, s, sector);
        return;
    }
    if !s.running {
        return;
    }
    // The chip raises one interrupt per word, so keep going while it has more, up to the FIFO's depth.
    for _ in 0..MAX_WORDS_PER_TICK {
        if (api.rf_reg_read)(R_STATUS) & 1 == 0 {
            break;
        }
        // Writing the interrupt register latches the flags; the read returns them.
        w(api, R_INT, 0);
        let irq = (api.rf_reg_read)(R_INT);
        s.irq = irq;
        s.dirty = true;
        let mut flags = 0;
        let mut word = 0;
        if irq & IRQ_SYNC != 0 {
            s.syncs = s.syncs.saturating_add(1);
            flags |= FLAG_SYNC;
        }
        if irq & (IRQ_WORD | IRQ_RX_DONE) != 0 {
            word = (api.rf_reg_read)(R_FIFO);
            s.words = s.words.saturating_add(1);
            s.last = [s.last[1], s.last[2], s.last[3], word];
            flags |= FLAG_WORD;
            if irq & IRQ_RX_DONE != 0 {
                flags |= FLAG_DONE;
            }
        }
        if flags == 0 {
            break;
        }
        if s.cap == Cap::Recording {
            let rec = record((api.uptime_100us)(), word, flags, s.rssi);
            if !s.rec.push(rec, &mut |addr, page| cap_write(api, addr, page)) {
                s.cap = Cap::Full;
            }
        }
    }
}

fn draw(api: &Api, s: &mut State) {
    // The RSSI line moves constantly, so it is redrawn every frame; the rest only when something changed.
    let raw = (api.read_rssi)();
    s.rssi = raw;
    let dbm = raw as i32 - 160; // datasheet: REG_67[8:0]/2 - 160; read_rssi already returns the halved value
    let mut b = [0u8; 28];
    let mut n = 0;
    n = put(&mut b, n, b"RSSI ");
    n = put_i(&mut b, n, dbm);
    n = put(&mut b, n, b" dBm  raw ");
    n = put_u(&mut b, n, raw as u32);
    line(api, 3, &b[..n], WHITE);
    if !s.dirty {
        return;
    }
    s.dirty = false;

    (api.fill_rect)(0, 0, SCREEN_W, LINE_H as u16, TITLE_BG);
    text(api, 2, 1, b"FSK PROBE  RX ONLY", WHITE, TITLE_BG);

    n = 0;
    n = put_freq(&mut b, n, FREQS[s.freq]);
    n = put(&mut b, n, b"  ");
    n = put_u(&mut b, n, BAUDS[s.baud] as u32);
    n = put(&mut b, n, b" baud");
    line(api, 1, &b[..n], WHITE);

    n = 0;
    let colour = match s.cap {
        Cap::Erasing(sector) => {
            n = put(&mut b, n, b"ERASING ");
            n = put_u(&mut b, n, sector as u32);
            n = put(&mut b, n, b"/");
            n = put_u(&mut b, n, CAP_SECTORS as u32);
            YELLOW
        }
        Cap::Recording | Cap::Full => {
            n = put(&mut b, n, if s.cap == Cap::Full { b"REC FULL " } else { b"REC " });
            n = put_u(&mut b, n, s.rec.count());
            n = put(&mut b, n, b"/");
            n = put_u(&mut b, n, CAP_RECORDS);
            RED
        }
        Cap::Idle => {
            n = put(&mut b, n, if s.running { b"MODEM ON" } else { b"MODEM OFF" });
            if s.saved > 0 {
                n = put(&mut b, n, b"  SAVED ");
                n = put_u(&mut b, n, s.saved);
            }
            if s.running { GREEN } else { GRAY }
        }
    };
    line(api, 2, &b[..n], colour);

    n = 0;
    n = put(&mut b, n, b"SYNC ");
    n = put_hex(&mut b, n, SYNCS[s.sync].0);
    n = put(&mut b, n, b" BAUDREG ");
    n = put_hex(&mut b, n, baud_reg(BAUDS[s.baud]));
    line(api, 4, &b[..n], WHITE);

    n = 0;
    n = put(&mut b, n, b"STAT ");
    n = put_hex(&mut b, n, s.status);
    n = put(&mut b, n, b" IRQ ");
    n = put_hex(&mut b, n, s.irq);
    line(api, 5, &b[..n], WHITE);

    n = 0;
    n = put(&mut b, n, b"SYNCS ");
    n = put_u(&mut b, n, s.syncs as u32);
    n = put(&mut b, n, b" WORDS ");
    n = put_u(&mut b, n, s.words as u32);
    line(api, 6, &b[..n], YELLOW);

    n = 0;
    for (i, wd) in s.last.iter().enumerate() {
        if i > 0 {
            n = put(&mut b, n, b" ");
        }
        n = put_hex(&mut b, n, *wd);
    }
    line(api, 7, &b[..n], WHITE);

    let reg = PEEK[s.peek];
    n = 0;
    n = put(&mut b, n, b"REG ");
    n = put_hex2(&mut b, n, reg);
    n = put(&mut b, n, b" = ");
    n = put_hex(&mut b, n, (api.rf_reg_read)(reg));
    line(api, 8, &b[..n], WHITE);

    line(api, 9, b"1-4 baud  5-9 sync", GRAY);
    line(api, 10, b"MENU on/off  * freq", GRAY);
    line(api, 11, b"# record  UP/DN reg  EXIT", GRAY);
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, 0);
}

fn line(api: &Api, row: i16, s: &[u8], fg: u16) {
    let y = row * LINE_H + 2;
    (api.fill_rect)(0, y, 2, LINE_H as u16, BLACK);
    let end = (api.draw_text)(2, y, s.as_ptr(), s.len() as u16, fg, BLACK, 0);
    (api.fill_rect)(end as i16, y, SCREEN_W.saturating_sub(end), LINE_H as u16, BLACK);
}

fn put(b: &mut [u8], n: usize, s: &[u8]) -> usize {
    let k = s.len().min(b.len() - n);
    b[n..n + k].copy_from_slice(&s[..k]);
    n + k
}

fn put_u(b: &mut [u8], n: usize, mut v: u32) -> usize {
    let mut d = [0u8; 10];
    let mut i = d.len();
    loop {
        i -= 1;
        d[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    put(b, n, &d[i..])
}

fn put_i(b: &mut [u8], n: usize, v: i32) -> usize {
    if v < 0 {
        let n = put(b, n, b"-");
        put_u(b, n, v.unsigned_abs())
    } else {
        put_u(b, n, v as u32)
    }
}

fn put_hex(b: &mut [u8], n: usize, v: u16) -> usize {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let d = [H[(v >> 12) as usize & 15], H[(v >> 8) as usize & 15], H[(v >> 4) as usize & 15], H[v as usize & 15]];
    put(b, n, &d)
}

fn put_hex2(b: &mut [u8], n: usize, v: u8) -> usize {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    put(b, n, &[H[(v >> 4) as usize & 15], H[v as usize & 15]])
}

/// `144.390` style (MHz with three decimals).
fn put_freq(b: &mut [u8], n: usize, hz: u32) -> usize {
    let n = put_u(b, n, hz / 1_000_000);
    let n = put(b, n, b".");
    let k = hz / 1_000 % 1_000;
    let n = put(b, n, &[b'0' + (k / 100) as u8, b'0' + (k / 10 % 10) as u8, b'0' + (k % 10) as u8]);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baud_register_follows_the_datasheet_formula() {
        // 4646 is what the working TEXT app writes for its 450 baud link.
        assert_eq!(baud_reg(450), 4646);
        assert_eq!(baud_reg(1200), 12389);
        assert_eq!(baud_reg(2400), 24779);
        assert_eq!(baud_reg(4800), 49557);
    }

    #[test]
    fn every_register_the_app_writes_is_on_the_allow_list() {
        for r in [R_INT, R_IRQ_EN, R_FSK, R_CTRL, R_SYNC1, R_SYNC2, R_CRC, R_SIZE, R_THRESH, R_FIFO, R_GAIN, R_BAUD] {
            assert!(bf5rh_abi::rf_reg_writable(r), "0x{r:02X} is not writable");
        }
    }

    #[test]
    fn capture_range_is_clear_of_everything_else_in_the_flash() {
        // app slots end at 0x194000, the satellite table follows (5120 bytes); extras table at 0x1E0000, TEXT log 0x1F0000
        assert!(CAP_ADDR >= 0x19_4000 + 5120);
        assert!(CAP_ADDR + CAP_BYTES <= 0x1E_0000 - 0x1_0000);
        assert_eq!(CAP_ADDR % SECTOR, 0);
        assert_eq!(CAP_BYTES % SECTOR, 0);
        assert_eq!(CAP_SECTORS as u32 * SECTOR, CAP_BYTES);
        assert_eq!(CAP_RECORDS, 16352);
    }

    #[test]
    fn recorder_never_writes_outside_the_range_however_much_it_is_given() {
        let mut r = Recorder::NEW;
        let mut writes: Vec<u32> = Vec::new();
        let mut accepted = 0u32;
        for i in 0..CAP_RECORDS * 3 {
            if r.push(record(i, i as u16, FLAG_WORD, 40), &mut |a, _| writes.push(a)) {
                accepted += 1;
            }
        }
        r.flush(&mut |a, _| writes.push(a));
        assert_eq!(accepted, CAP_RECORDS);
        assert_eq!(r.count(), CAP_RECORDS);
        assert_eq!(writes.len() as u32, CAP_PAGES as u32 - 1);
        assert_eq!(writes[0], CAP_ADDR + PAGE as u32, "page 0 is the header's");
        assert_eq!(*writes.last().unwrap(), CAP_ADDR + CAP_BYTES - PAGE as u32);
        for (i, a) in writes.iter().enumerate() {
            assert_eq!(*a, CAP_ADDR + (i as u32 + 1) * PAGE as u32, "pages in order, each written once");
            assert!(in_capture_range(*a, PAGE as u32));
        }
    }

    #[test]
    fn a_partial_page_is_written_once_and_its_unused_part_stays_erased() {
        let mut r = Recorder::NEW;
        let mut pages: Vec<(u32, [u8; PAGE])> = Vec::new();
        for i in 0..3 {
            assert!(r.push(record(100 + i, 0xABCD, FLAG_SYNC | FLAG_WORD, 300), &mut |a, p| pages.push((a, *p))));
        }
        assert!(pages.is_empty());
        r.flush(&mut |a, p| pages.push((a, *p)));
        r.flush(&mut |a, p| pages.push((a, *p)));
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].0, CAP_ADDR + PAGE as u32);
        assert_eq!(&pages[0].1[..REC], &[100, 0, 0, 0, 0xCD, 0xAB, 3, 255]);
        assert!(pages[0].1[3 * REC..].iter().all(|&b| b == 0xFF));
        assert_eq!(r.count(), 3);
    }

    #[test]
    fn range_check_refuses_anything_outside() {
        assert!(in_capture_range(CAP_ADDR, PAGE as u32));
        assert!(in_capture_range(CAP_ADDR + CAP_BYTES - 256, 256));
        assert!(!in_capture_range(CAP_ADDR + CAP_BYTES - 255, 256));
        assert!(!in_capture_range(CAP_ADDR - 1, 1));
        assert!(!in_capture_range(0x1E_0000, 256));
        assert!(!in_capture_range(CAP_ADDR, CAP_BYTES + 1));
        assert!(!in_capture_range(u32::MAX, 2));
        assert_eq!(page_addr(0), None);
        assert_eq!(page_addr(CAP_PAGES), None);
        assert_eq!(page_addr(u16::MAX), None);
    }

    #[test]
    fn text_helpers() {
        let mut b = [0u8; 28];
        let n = put_freq(&mut b, 0, 144_390_000);
        assert_eq!(&b[..n], b"144.390");
        let n = put_freq(&mut b, 0, 434_714_000);
        assert_eq!(&b[..n], b"434.714");
        let n = put_i(&mut b, 0, -105);
        assert_eq!(&b[..n], b"-105");
        let n = put_hex(&mut b, 0, 0x7E7E);
        assert_eq!(&b[..n], b"7E7E");
        let n = put_hex(&mut b, 0, 0x00AB);
        assert_eq!(&b[..n], b"00AB");
        let n = put_u(&mut b, 0, 0);
        assert_eq!(&b[..n], b"0");
    }
}
