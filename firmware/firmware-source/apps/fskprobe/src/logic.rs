//! FSK probe: a hardware bring-up tool for the radio's FSK modem. It answers "does this chip's modem hear anything,
//! and what do its registers say?" before any modem app (APRS, Horus, RS41) is trusted.
//!
//! Receive-only. It never keys the transmitter and never calls `api.tone_on` or `api.set_ptt`. Chip access goes
//! through `api.rf_reg_read` / `api.rf_reg_write`; the core refuses writes outside the modem allow-list
//! (`bf5rh_abi::RF_WRITABLE`), and this app only ever writes registers on that list.
//!
//! Register meanings come from Fudan's "FD6818B Registers Definition" (2025/1/11); the table in HARDWARE.md says, field
//! by field, what is confirmed and what is not. Still UNVERIFIED on the YP07's FD6818B: the crystal (the baud formula
//! depends on it), the sync words and their bit order, the 11-bit length field, what a given almost-full threshold
//! delivers, and that the modem hears a real signal at all. Finding that out is the point of this app.
//!
//! The first capture from the radio (noise, 404.800 MHz, 4800 baud) held one sync interrupt and, 119.7 ms later, one
//! FIFO word with the done flag. Two faults in this app's setup explain it (HARDWARE.md, "FSK modem registers"):
//! REG_59 bit 3 (4-byte sync) was never set, so the chip matched only 16 bits, and the second REG_59 write left bit 14
//! (clear RX FIFO) high, which the register document says clears the FIFO. `R_CTRL` is now built by `ctrl()` from named
//! bits, and the options below let the owner switch the readings the sources leave open.

use bf5rh_abi::{Api, AppEvent, AppResult};

const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;
const KEY_POUND: u8 = 11;
const KEY_AB: u8 = 17;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const TITLE_BG: u16 = 0x5800;
const GREEN: u16 = 0x07E0;
const YELLOW: u16 = 0xFFE0;
const RED: u16 = 0xF800;

const R_INT: u8 = 0x02;
const R_SYNC_SEEN: u8 = 0x0B; // read only: bit 6 FSK sync (positive) found, bit 7 FSK sync (negative) found
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

// REG_59 (FD6818B register document; TEXT and the K5/K6 drivers write the same bits)
const CTRL_CLEAR_TX: u16 = 1 << 15;
const CTRL_CLEAR_RX: u16 = 1 << 14; // 1 = clear; it must be written back to 0 or the FIFO stays empty
const CTRL_CLEAR: u16 = CTRL_CLEAR_TX | CTRL_CLEAR_RX;
const CTRL_RX_EN: u16 = 1 << 12;
/// What the core writes to REG_59 when it is done with the modem (its `FSK_CONTROL_BASE`).
const CTRL_IDLE: u16 = 0x0028;
const CTRL_INVERT_RX: u16 = 1 << 10;
const CTRL_SYNC4: u16 = 1 << 3; // 0 = 2 sync bytes (REG_5A), 1 = 4 sync bytes (REG_5A, REG_5B)
// Scramble (bit 13) and TX bits (11, 9) are never set: this app does not transmit and the RS41 is not scrambled
// by the modem's own scrambler.

/// Registers the peek view cycles through. Reading is unrestricted; this is just the interesting set. Never REG_5F: a
/// read of it takes a word out of the FIFO. REG_0B, 0x70 are at the end so the older entries keep their position.
const PEEK: [u8; 16] = [
    0x02, 0x0C, 0x2B, 0x30, 0x3F, 0x40, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x72, 0x0B, 0x70,
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

// ---- options -------------------------------------------------------------------------------------------------
// Where the sources leave two readings open, the owner picks (key AB selects an item, key 0 changes it) and the
// capture header records the choice. Defaults are the reading that best fits the sources.
//
//   LEN   how long the chip is told a packet is (REG_5D): FRAME = the RS41 frame minus the sync bytes (11-bit field,
//         UNVERIFIED that the chip implements the top 3 bits), 256 = the largest an 8-bit field can say, 2048 = the
//         largest the 11-bit field can say. In every mode the app counts words itself and ends the frame at
//         `frame_words`, so a chip that stops early (256) or never stops (2048) still gives whole frames.
//   TH    REG_5E bits 2:0, the RX FIFO almost-full threshold, and so how many words the app takes per interrupt
//         (TEXT: 1 word per interrupt at 1, confirmed on this chip; the K5 firmware takes 4 per interrupt at the
//         chip's default of 4). Taking fewer than the FIFO holds loses nothing but time; taking more reads empties.
//   INV   REG_59 bit 10, invert received data. RS41 polarity against this chip is unknown. REG_0B bits 7:6 (negative
//         and positive sync found) are recorded with every sync, which says which polarity the chip matched.
//   BW    REG_58 bits 3:1 RX bandwidth: 000 "FSK 1.2K", 100 "FSK 2.4K" (the widest the document lists). 4800 baud
//         needs the wider one on paper; UNVERIFIED either way.
//   SYNC  REG_59 bit 3, 4 or 2 sync bytes.
const RS41_FRAME: u16 = 320;
const LEN_FRAME: u8 = 0;
const LEN_256: u8 = 1;
const LEN_MAX: u8 = 2;
const N_ITEMS: u8 = 5;

#[derive(Clone, Copy, PartialEq)]
struct Opts {
    len: u8,
    thr4: bool,
    inv: bool,
    wide: bool,
    sync2: bool,
}

const OPTS_DEFAULT: Opts = Opts { len: LEN_FRAME, thr4: false, inv: false, wide: true, sync2: false };

impl Opts {
    fn sync_bytes(&self) -> u16 {
        if self.sync2 { 2 } else { 4 }
    }
    /// Bytes after the sync that make up one frame (the chip does not deliver the sync bytes themselves).
    fn frame_bytes(&self) -> u16 {
        RS41_FRAME - self.sync_bytes()
    }
    fn frame_words(&self) -> u16 {
        self.frame_bytes() / 2
    }
    /// Bytes the chip is told to count before it raises the done flag.
    fn chip_bytes(&self) -> u16 {
        match self.len {
            LEN_FRAME => self.frame_bytes(),
            LEN_256 => 256,
            _ => 2048,
        }
    }
    fn words_per_irq(&self) -> u8 {
        if self.thr4 { 4 } else { 1 }
    }
    /// REG_59 with receive off: the bits that persist across a restart.
    fn ctrl(&self) -> u16 {
        (if self.sync2 { 0 } else { CTRL_SYNC4 }) | if self.inv { CTRL_INVERT_RX } else { 0 }
    }
    fn fsk(&self) -> u16 {
        0x0301 | if self.wide { 0b100 << 1 } else { 0 }
    }
    fn thresh(&self) -> u16 {
        (64 << 3) | self.words_per_irq() as u16
    }
    fn size(&self) -> u16 {
        len_field(self.chip_bytes())
    }
    /// The value the capture header records: len in bits 1:0, then thr4, inv, wide, sync2.
    fn bits(&self) -> u8 {
        self.len | (self.thr4 as u8) << 2 | (self.inv as u8) << 3 | (self.wide as u8) << 4 | (self.sync2 as u8) << 5
    }
    fn step(&mut self, item: u8) {
        match item {
            0 => self.len = (self.len + 1) % 3,
            1 => self.thr4 = !self.thr4,
            2 => self.inv = !self.inv,
            3 => self.wide = !self.wide,
            _ => self.sync2 = !self.sync2,
        }
    }
}

/// REG_5D: bits 15:8 are the low 8 bits and bits 7:5 the high 3 of an 11-bit length, written as bytes minus one
/// ("0xF means 16 bytes", register document; the K5 firmware writes 0x4700 for 72 bytes). Clamped to 1..=2048.
fn len_field(bytes: u16) -> u16 {
    let n = bytes.clamp(1, 2048) - 1;
    ((n & 0xFF) << 8) | (((n >> 8) & 7) << 5)
}

/// Words to take out of the FIFO when the done flag fires: what the chip was told to count and the app has not read yet.
/// The FIFO holds 8 words, so never more than 8; nothing when no sync was seen (the count is unknown).
fn tail_words(o: &Opts, in_frame: bool, got: u16) -> u8 {
    if !in_frame {
        return 0;
    }
    (o.chip_bytes() / 2).saturating_sub(got).min(8) as u8
}

/// Word time in 100 us units: 16 bits at `baud`.
fn word_100us(baud: u16) -> u32 {
    160_000 / baud.max(1) as u32
}

/// How long a frame may go quiet before the app gives up on it and listens for the next sync.
fn silence_100us(baud: u16) -> u32 {
    (8 * word_100us(baud)).max(1000)
}

// ---- capture --------------------------------------------------------------------------------------------------
// `#` records what the modem delivers into a fixed range of the radio's data flash, for `probe_capture.py` to
// download. The range is 0x1B0000-0x1D0000: past the app slots and the satellite table, with 64 KB of unused flash
// between its end and the channel extras table at 0x1E0000. Nothing else uses it.
//
//   page 0            header, version 2: "FSKCAP2\0", frequency Hz u32, baud u16, sync halves 2 x u16, baud
//                     register u16, start time u32 (100 us units), record size u8 (bytes 0-24, as version 1),
//                     then: header version u8 (2), options u8 (Opts::bits), words per interrupt u8, and the values
//                     the app wrote to REG_3F, 58, 59 (receive running), 5C, 5D, 5E, 70 (7 x u16), frame words u16;
//                     the rest erased. Version 1 ("FSKCAP1\0") stops after the record size.
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
const CAP_MAGIC: &[u8; 8] = b"FSKCAP2\0";
const HEADER_VERSION: u8 = 2;

const FLAG_SYNC: u8 = 1;
const FLAG_WORD: u8 = 2;
const FLAG_DONE: u8 = 4;
/// On a sync record: REG_0B said the chip matched the sync as sent (POS) or inverted (NEG).
const FLAG_POS: u8 = 8;
const FLAG_NEG: u8 = 16;
/// On the last word of a frame the app ended (by count) before re-arming the receiver.
const FLAG_FRAME: u8 = 32;
/// On a word read after the done flag, to empty what the length counter left in the FIFO.
const FLAG_TAIL: u8 = 64;

/// The most interrupts one tick will service before giving the rest of the firmware its turn. The FIFO holds 8 words,
/// and a tick that cannot keep up shows in the capture as a gap, not as silent loss.
const MAX_IRQS_PER_TICK: u8 = 16;
/// The most words one interrupt can yield: the threshold's worth, then the tail.
const MAX_WORDS_PER_IRQ: usize = 12;

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
    h[25] = HEADER_VERSION;
    h[26] = s.opts.bits();
    h[27] = s.opts.words_per_irq();
    let regs = [IRQ_RX_DONE | IRQ_WORD | IRQ_SYNC, s.opts.fsk(), s.opts.ctrl() | CTRL_RX_EN, CRC_VALUE, s.opts.size(), s.opts.thresh(), GAIN_VALUE];
    for (i, r) in regs.iter().enumerate() {
        h[28 + 2 * i..30 + 2 * i].copy_from_slice(&r.to_le_bytes());
    }
    h[42..44].copy_from_slice(&s.opts.frame_words().to_le_bytes());
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
    opts: Opts,
    /// Which option key 0 changes (AB moves it).
    item: u8,
    /// A sync was seen and the frame it started is not finished: `fwords` words have come since.
    in_frame: bool,
    fwords: u16,
    frames: u16,
    /// Frames given up on because the words stopped before the frame was complete.
    cut: u16,
    last_word_t: u32,
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
    opts: OPTS_DEFAULT,
    item: 0,
    in_frame: false,
    fwords: 0,
    frames: 0,
    cut: 0,
    last_word_t: 0,
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

/// Same values as TEXT writes for these (no chip CRC; TONE2 on with its gain, which the FSK modem needs).
const CRC_VALUE: u16 = 0x5625;
const GAIN_VALUE: u16 = (1 << 7) | 96;

/// Writes every modem register from the current settings and starts listening for a sync.
fn modem_config(api: &Api, s: &mut State) {
    w(api, R_GAIN, GAIN_VALUE);
    w(api, R_BAUD, baud_reg(BAUDS[s.baud]));
    w(api, R_FSK, s.opts.fsk());
    w(api, R_SYNC1, SYNCS[s.sync].0);
    w(api, R_SYNC2, SYNCS[s.sync].1);
    w(api, R_CRC, CRC_VALUE);
    w(api, R_SIZE, s.opts.size());
    w(api, R_THRESH, s.opts.thresh());
    w(api, R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD | IRQ_SYNC);
    rearm(api, s);
}

/// The sequence TEXT uses between packets: clear both FIFOs with receive off, drop the clear bits, clear the
/// interrupt flags, then turn receive on. The clear bit must go back to 0 or the FIFO stays empty.
fn rearm(api: &Api, s: &mut State) {
    let base = s.opts.ctrl();
    w(api, R_CTRL, base | CTRL_CLEAR);
    w(api, R_CTRL, base);
    w(api, R_INT, 0);
    w(api, R_CTRL, base | CTRL_RX_EN);
    s.in_frame = false;
    s.fwords = 0;
}

fn modem_on(api: &Api, s: &mut State) {
    modem_config(api, s);
    s.running = true;
    s.syncs = 0;
    s.words = 0;
    s.frames = 0;
    s.cut = 0;
    s.last = [0; 4];
}

/// Also ends a capture: nothing records while the modem is off. Leaves the chip as the core's own FSK exit does: FSK
/// enable and TONE2 off, REG_59 at its idle value with the FIFO clear bits low.
fn modem_off(api: &Api, s: &mut State) {
    w(api, R_IRQ_EN, 0);
    w(api, R_CTRL, CTRL_CLEAR);
    w(api, R_CTRL, CTRL_IDLE);
    w(api, R_FSK, 0);
    w(api, R_GAIN, 0);
    s.running = false;
    s.in_frame = false;
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
        KEY_AB => s.item = (s.item + 1) % N_ITEMS,
        0 => {
            s.opts.step(s.item);
            if s.running {
                modem_config(api, s);
            }
        }
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
                rearm(api, s);
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
    service(api, s);
    let now = (api.uptime_100us)();
    if s.in_frame && now.wrapping_sub(s.last_word_t) > silence_100us(BAUDS[s.baud]) {
        // The words stopped before the frame was whole (noise sync, or the signal went): listen for the next sync.
        s.cut = s.cut.saturating_add(1);
        s.dirty = true;
        rearm(api, s);
    }
}

/// Services every interrupt the chip has raised, up to `MAX_IRQS_PER_TICK`. An almost-full interrupt means the FIFO
/// holds at least its threshold of words (REG_5E), so that many are read; the done flag means the length counter ran
/// out, so the rest of the counted words are read; after either, the next sync is waited for again.
fn service(api: &Api, s: &mut State) {
    for _ in 0..MAX_IRQS_PER_TICK {
        if (api.rf_reg_read)(R_STATUS) & 1 == 0 {
            break;
        }
        // Writing the interrupt register latches the flags; the read returns them.
        w(api, R_INT, 0);
        let irq = (api.rf_reg_read)(R_INT);
        s.irq = irq;
        s.dirty = true;
        let now = (api.uptime_100us)();
        let mut first = 0u8;
        if irq & IRQ_SYNC != 0 {
            s.syncs = s.syncs.saturating_add(1);
            first |= FLAG_SYNC;
            let seen = (api.rf_reg_read)(R_SYNC_SEEN);
            if seen & (1 << 6) != 0 {
                first |= FLAG_POS;
            }
            if seen & (1 << 7) != 0 {
                first |= FLAG_NEG;
            }
            s.in_frame = true;
            s.fwords = 0;
            s.last_word_t = now;
        }
        let mut got = [0u16; MAX_WORDS_PER_IRQ];
        let mut n = 0;
        if irq & IRQ_WORD != 0 {
            for _ in 0..s.opts.words_per_irq() {
                got[n] = (api.rf_reg_read)(R_FIFO);
                n += 1;
            }
        }
        let body = n;
        let done = irq & IRQ_RX_DONE != 0;
        if done {
            // `fwords` does not count this interrupt's words yet.
            let seen = s.fwords.saturating_add(body as u16);
            for _ in 0..tail_words(&s.opts, s.in_frame, seen).min((MAX_WORDS_PER_IRQ - n) as u8) {
                got[n] = (api.rf_reg_read)(R_FIFO);
                n += 1;
            }
        }
        if n > 0 {
            s.last_word_t = now;
            s.words = s.words.saturating_add(n as u16);
            s.fwords = s.fwords.saturating_add(n as u16);
            for &x in &got[..n] {
                s.last = [s.last[1], s.last[2], s.last[3], x];
            }
        }
        // The frame is over when the app's own count says so, or when the chip said done and the app is not
        // carrying on past it (LEN 256 ends early on purpose).
        let whole = s.in_frame && s.fwords >= s.opts.frame_words();
        let chip_ended = done && !(s.opts.len == LEN_256 && s.in_frame && !whole);
        if first == 0 && n == 0 && !done {
            break;
        }
        if s.cap == Cap::Recording {
            for i in 0..n.max(1) {
                let mut flags = if n > 0 { FLAG_WORD } else { 0 };
                if i == 0 {
                    flags |= first;
                }
                if i >= body && n > 0 {
                    flags |= FLAG_TAIL;
                }
                if i + 1 >= n.max(1) {
                    if done {
                        flags |= FLAG_DONE;
                    }
                    if whole {
                        flags |= FLAG_FRAME;
                    }
                }
                let word = if n > 0 { got[i] } else { 0 };
                let rec = record(now, word, flags, s.rssi);
                if !s.rec.push(rec, &mut |addr, page| cap_write(api, addr, page)) {
                    s.cap = Cap::Full;
                    break;
                }
            }
        }
        if whole {
            s.frames = s.frames.saturating_add(1);
        }
        if whole || chip_ended {
            rearm(api, s);
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
    n = put(&mut b, n, b"SYN ");
    n = put_u(&mut b, n, s.syncs as u32);
    n = put(&mut b, n, b" WRD ");
    n = put_u(&mut b, n, s.words as u32);
    n = put(&mut b, n, b" FRM ");
    n = put_u(&mut b, n, s.frames as u32);
    n = put(&mut b, n, b" CUT ");
    n = put_u(&mut b, n, s.cut as u32);
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
    n = put(&mut b, n, b"  UP/DN");
    line(api, 8, &b[..n], WHITE);

    opts_line(api, 9, s);
    line(api, 10, b"1-4 baud 5-9 sync * freq", GRAY);
    line(api, 11, b"MENU run # rec AB/0 option", GRAY);
}

/// The five options on one row, the one key 0 will change in yellow.
fn opts_line(api: &Api, row: i16, s: &State) {
    let y = row * LINE_H + 2;
    (api.fill_rect)(0, y, SCREEN_W, LINE_H as u16, BLACK);
    let o = &s.opts;
    let mut x = 2u16;
    for item in 0..N_ITEMS {
        let mut b = [0u8; 12];
        let mut n = 0;
        n = match item {
            0 => {
                let n = put(&mut b, n, b"LEN");
                put_u(&mut b, n, o.chip_bytes() as u32)
            }
            1 => {
                let n = put(&mut b, n, b"TH");
                put_u(&mut b, n, o.words_per_irq() as u32)
            }
            2 => put(&mut b, n, if o.inv { b"INV+" } else { b"INV-" }),
            3 => put(&mut b, n, if o.wide { b"BW2.4" } else { b"BW1.2" }),
            _ => {
                let n = put(&mut b, n, b"S");
                put_u(&mut b, n, o.sync_bytes() as u32)
            }
        };
        n = put(&mut b, n, b" ");
        let fg = if item == s.item { YELLOW } else { WHITE };
        x = (api.draw_text)(x as i16, y, b.as_ptr(), n as u16, fg, BLACK, 0);
    }
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

// Read-only views for the PC simulator; not part of the radio build.
#[cfg(not(target_os = "none"))]
pub fn debug_counts() -> (u16, u16, u16, u16) {
    let s = st();
    (s.syncs, s.words, s.frames, s.cut)
}

#[cfg(not(target_os = "none"))]
pub fn debug_last() -> [u16; 4] {
    st().last
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
        // R_SYNC_SEEN (REG_0B) is only read, so it is not here.
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
    fn receive_control_word_has_four_byte_sync_and_never_holds_a_fifo_clear() {
        // The first capture's faults: bit 3 (4-byte sync) was missing and bit 14 (clear RX FIFO) stayed set.
        let o = OPTS_DEFAULT;
        assert_eq!(o.ctrl(), CTRL_SYNC4);
        assert_eq!(o.ctrl() | CTRL_RX_EN, 0x1008, "the value TEXT leaves receive running with");
        for len in 0..3 {
            for bits in 0..16u8 {
                let o = Opts { len, thr4: bits & 1 != 0, inv: bits & 2 != 0, wide: bits & 4 != 0, sync2: bits & 8 != 0 };
                let running = o.ctrl() | CTRL_RX_EN;
                assert_eq!(running & (CTRL_CLEAR | (1 << 13) | (1 << 11) | (1 << 9)), 0, "no clear, scramble or TX bit");
                assert_eq!(running & CTRL_SYNC4 != 0, !o.sync2);
                assert_eq!(running & CTRL_INVERT_RX != 0, o.inv);
                assert_eq!(running & 0x0300, 0, "bit 8 is not in the register document; it is no longer set");
            }
        }
        assert_eq!(Opts { sync2: true, ..OPTS_DEFAULT }.ctrl(), 0);
    }

    #[test]
    fn length_field_is_bytes_minus_one_in_eleven_bits() {
        // "0xF means 16 bytes" (register document); the K5 firmware writes 0x4700 for 72 bytes; the K6 0x0F00 for 8 words.
        assert_eq!(len_field(16), 0x0F00);
        assert_eq!(len_field(72), 0x4700);
        assert_eq!(len_field(256), 0xFF00);
        assert_eq!(len_field(257), 0x0020, "the ninth bit goes to bit 5");
        assert_eq!(len_field(316), 0x3B20);
        assert_eq!(len_field(320), 0x3F20);
        assert_eq!(len_field(2048), 0xFFE0);
        assert_eq!(len_field(0), 0x0000);
        assert_eq!(len_field(65535), 0xFFE0, "clamped");
        for b in 1..=2048u16 {
            let f = len_field(b);
            assert_eq!(f & 0x001F, 0, "bits 4:0 are never touched");
            assert_eq!(((f >> 8) | ((f >> 5) & 7) << 8) + 1, b);
        }
    }

    #[test]
    fn options_give_the_register_values_the_sources_support() {
        let o = OPTS_DEFAULT;
        assert_eq!((o.sync_bytes(), o.frame_bytes(), o.frame_words()), (4, 316, 158));
        assert_eq!(o.size(), 0x3B20);
        assert_eq!(o.thresh(), (64 << 3) | 1, "TX threshold at its default of 64, RX at one word, as TEXT");
        assert_eq!(o.fsk(), 0x0301 | 0x0008, "TEXT's value with the 2.4K receive bandwidth");
        let narrow = Opts { wide: false, ..o };
        assert_eq!(narrow.fsk(), 0x0301, "exactly TEXT's and the first capture's value");
        let two = Opts { sync2: true, ..o };
        assert_eq!((two.frame_bytes(), two.frame_words()), (318, 159));
        assert_eq!(Opts { len: LEN_256, ..o }.size(), 0xFF00);
        assert_eq!(Opts { len: LEN_MAX, ..o }.size(), 0xFFE0);
        assert_eq!(Opts { thr4: true, ..o }.thresh() & 7, 4);
        assert_eq!(Opts { thr4: true, ..o }.words_per_irq(), 4);
        // Every item steps through its values and comes back.
        for item in 0..N_ITEMS {
            let mut x = o;
            let mut seen = 0;
            loop {
                x.step(item);
                seen += 1;
                if x == o {
                    break;
                }
                assert!(seen < 4);
            }
            assert_eq!(seen, if item == 0 { 3 } else { 2 });
        }
        // The header's options byte tells every setting apart.
        let mut all = std::collections::HashSet::new();
        for len in 0..3 {
            for bits in 0..16u8 {
                let o = Opts { len, thr4: bits & 1 != 0, inv: bits & 2 != 0, wide: bits & 4 != 0, sync2: bits & 8 != 0 };
                assert!(all.insert(o.bits()));
            }
        }
    }

    #[test]
    fn tail_after_done_is_what_the_counter_ran_out_on_and_never_more_than_the_fifo() {
        let o = OPTS_DEFAULT;
        assert_eq!(tail_words(&o, true, 158), 0, "everything already read");
        assert_eq!(tail_words(&o, true, 156), 2, "two words left under a threshold of 4");
        assert_eq!(tail_words(&o, true, 0), 8, "never more than the FIFO holds");
        assert_eq!(tail_words(&o, false, 0), 0, "no sync seen, so the count is unknown");
        assert_eq!(tail_words(&Opts { len: LEN_256, ..o }, true, 127), 1);
        assert_eq!(tail_words(&Opts { len: LEN_256, ..o }, true, 200), 0);
    }

    #[test]
    fn timeouts_scale_with_baud() {
        assert_eq!(word_100us(4800), 33);
        assert_eq!(word_100us(450), 355);
        assert_eq!(silence_100us(4800), 1000, "never shorter than 100 ms");
        assert_eq!(silence_100us(450), 2840);
    }

    #[test]
    fn peek_never_reads_the_fifo() {
        assert!(!PEEK.contains(&R_FIFO), "a read of REG_5F takes a word out of the FIFO");
    }

    #[test]
    fn header_records_the_configuration() {
        let mut st = STATE_DEFAULT;
        st.opts = Opts { len: LEN_256, thr4: true, inv: true, wide: false, sync2: true };
        let h = header(&st, 0x1234_5678);
        assert_eq!(&h[..8], b"FSKCAP2\0");
        assert_eq!(&h[8..12], &404_800_000u32.to_le_bytes());
        assert_eq!(&h[12..14], &4800u16.to_le_bytes());
        assert_eq!(&h[14..18], &[0x6D, 0x08, 0x88, 0x53]);
        assert_eq!(&h[18..20], &49557u16.to_le_bytes());
        assert_eq!(&h[20..24], &0x1234_5678u32.to_le_bytes());
        assert_eq!(h[24], 8);
        assert_eq!(h[25], 2, "header version");
        assert_eq!(h[26], st.opts.bits());
        assert_eq!(h[26], 1 | 1 << 2 | 1 << 3 | 1 << 5);
        assert_eq!(h[27], 4, "words per interrupt");
        let reg = |i: usize| u16::from_le_bytes([h[28 + 2 * i], h[29 + 2 * i]]);
        assert_eq!(reg(0), 0x3002, "REG_3F");
        assert_eq!(reg(1), 0x0301, "REG_58");
        assert_eq!(reg(2), CTRL_INVERT_RX | CTRL_RX_EN, "REG_59 as receive runs");
        assert_eq!(reg(3), 0x5625, "REG_5C");
        assert_eq!(reg(4), 0xFF00, "REG_5D");
        assert_eq!(reg(5), (64 << 3) | 4, "REG_5E");
        assert_eq!(reg(6), 0x00E0, "REG_70");
        assert_eq!(u16::from_le_bytes([h[42], h[43]]), 159, "frame words");
        assert!(h[44..].iter().all(|&b| b == 0xFF), "the rest stays erased");
    }

    #[test]
    fn flags_keep_bit_seven_clear_so_an_erased_record_ends_the_capture() {
        let all = FLAG_SYNC | FLAG_WORD | FLAG_DONE | FLAG_POS | FLAG_NEG | FLAG_FRAME | FLAG_TAIL;
        assert_eq!(all, 0x7F);
        assert_eq!(record(1, 2, all, 3)[6], 0x7F);
        assert_eq!(record(1, 2, 0xFF, 3)[6], 0x7F, "masked even if a caller slips");
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
