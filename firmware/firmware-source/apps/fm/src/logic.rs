//! FM broadcast receiver (the RDA5807 tuner chip, reached through `api.fm_*`), 87.5 to 108.0 MHz.
//!
//! Two screens. The tuner shows the frequency in large digits, the station's name under it, step arrows either side,
//! and a tuning scale (saved stations marked) across the bottom. The station list is the shared list widget with the
//! station playing marked by a speaker.
//!
//! Keys, tuner:  UP/DOWN 0.1 MHz   hold UP/DOWN seek to the next signal   * / # previous / next saved station
//!               0-9 type a frequency (981 or 0981 = 98.1, 1015 = 101.5)   MENU station list   EXIT clear, then leave
//! Keys, list:   UP/DOWN move   MENU tune to it (on `+ SAVE THIS`: save the frequency)   * then MENU delete   EXIT back
//!
//! Receive-only. It never calls `api.tone_on` or `api.set_ptt`.
//!
//! Station names: a table in the data flash at `TBL_ADDR` (two 4 KB halves, see below), installed from a CSV by
//! `firmware/source-changes/fm_stations.py` and edited here (save, delete). Without a valid table the app shows the
//! core's bare 30-frequency list (`fm_channels_load`) and the first save or delete moves it into a table.
//!
//! The table. Each half is one flash sector: a 32-byte header record, then `SLOTS` (127) 32-byte station records.
//!   header  "FMST", version u8 = 1, reserved u8, generation u16 LE, 0xFF..., 0xA5 in the last byte
//!   record  frequency u16 LE in 0.1 MHz (0xFFFF = never written, 0x0000 = deleted), name 12 bytes, detail 16 bytes
//!           (ASCII, padded with 0xFF), flags u8 (0xFF), 0xA5 in the last byte
//! The tool writes half 0 with generation 1. The app only ever appends a record (a flash write can only clear bits)
//! or clears a record's frequency to delete it. When the half is full it copies the live records into the other half
//! (erased first) and writes that half's header last with the next generation: the half with the newer valid header is
//! the table, so a power cut at any point leaves either the old table or the new one. A record without its 0xA5 mark,
//! or with a frequency outside the band, is ignored.
//!
//! Every flash write and erase goes through `tbl_write` / `tbl_erase`, which refuse anything outside
//! `TBL_ADDR .. TBL_ADDR + TBL_BYTES` (the core only refuses addresses below the app area). UNVERIFIED on the radio.

use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KIND_REPEAT: u8 = 4;
const KEY_STAR: u8 = 10;
const KEY_POUND: u8 = 11;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;

pub const LO: u16 = 875;
pub const HI: u16 = 1080;
const DEFAULT: u16 = 1000;
const SEEK_TIMEOUT_100US: u32 = 30_000;
const TOAST_100US: u32 = 12_000;
const CORE_LIST: usize = 30;

// ---- the station table ---------------------------------------------------------------------------------------

pub const TBL_ADDR: u32 = 0x1D_8000;
pub const TBL_BYTES: u32 = 0x2000;
const SECTOR: u32 = 0x1000;
const REC: usize = 32;
pub const SLOTS: usize = 127;
const MAGIC: &[u8; 4] = b"FMST";
const MARK: u8 = 0xA5;
const NAME_LEN: usize = 12;

pub trait Nor {
    fn read(&mut self, addr: u32, out: &mut [u8]);
    fn write(&mut self, addr: u32, data: &[u8]) -> bool;
    fn erase(&mut self, addr: u32) -> bool;
}

fn in_table(addr: u32, len: u32) -> bool {
    addr >= TBL_ADDR && len <= TBL_BYTES && addr - TBL_ADDR <= TBL_BYTES - len
}

fn tbl_write<N: Nor>(n: &mut N, addr: u32, data: &[u8]) -> bool {
    in_table(addr, data.len() as u32) && n.write(addr, data)
}

fn tbl_erase<N: Nor>(n: &mut N, half: u32) -> bool {
    let addr = TBL_ADDR + half * SECTOR;
    half < 2 && in_table(addr, SECTOR) && n.erase(addr)
}

#[derive(PartialEq, Eq, Debug)]
pub enum Rec {
    Free,
    Dead,
    Live(u16),
}

pub fn parse_rec(r: &[u8; REC]) -> Rec {
    let f = u16::from_le_bytes([r[0], r[1]]);
    if f == 0xFFFF && r[REC - 1] == 0xFF {
        Rec::Free
    } else if r[REC - 1] == MARK && (LO..=HI).contains(&f) {
        Rec::Live(f)
    } else {
        Rec::Dead
    }
}

pub fn make_rec(freq: u16, name: &[u8], detail: &[u8]) -> [u8; REC] {
    let mut r = [0xFF; REC];
    r[..2].copy_from_slice(&freq.to_le_bytes());
    let (n, d) = (name.len().min(NAME_LEN), detail.len().min(16));
    r[2..2 + n].copy_from_slice(&name[..n]);
    r[14..14 + d].copy_from_slice(&detail[..d]);
    r[REC - 1] = MARK;
    r
}

fn header_gen(h: &[u8; REC]) -> Option<u16> {
    if &h[..4] == MAGIC && h[4] == 1 && h[REC - 1] == MARK {
        Some(u16::from_le_bytes([h[6], h[7]]))
    } else {
        None
    }
}

fn make_header(gen: u16) -> [u8; REC] {
    let mut h = [0xFF; REC];
    h[..4].copy_from_slice(MAGIC);
    h[4] = 1;
    h[6..8].copy_from_slice(&gen.to_le_bytes());
    h[REC - 1] = MARK;
    h
}

/// The live stations in table order, and where the table stands.
pub struct Stations {
    pub n: usize,
    pub fq: [u16; SLOTS],
    /// record number (1..=127) of station i in the active half; all 0 while the list is the core's bare one
    pub slot: [u8; SLOTS],
    /// a valid table is on the radio and `half`/`gen` say which
    pub have: bool,
    half: u8,
    gen: u16,
    /// the number of the last record that was ever written, live or dead (0 = none; `SLOTS` = the half is full)
    used: usize,
}

impl Stations {
    pub const NEW: Stations = Stations { n: 0, fq: [0; SLOTS], slot: [0; SLOTS], have: false, half: 0, gen: 0, used: 0 };

    pub fn find(&self, freq: u16) -> Option<usize> {
        self.fq[..self.n].iter().position(|&f| f == freq)
    }

    fn rec_addr(&self, rec: usize) -> u32 {
        TBL_ADDR + self.half as u32 * SECTOR + (rec * REC) as u32
    }
}

fn read_rec<N: Nor>(nor: &mut N, st: &Stations, i: usize) -> [u8; REC] {
    let mut r = [0xFF; REC];
    if st.have {
        nor.read(st.rec_addr(st.slot[i] as usize), &mut r);
        r
    } else {
        make_rec(st.fq[i], &[], &[])
    }
}

/// Reads the table. Leaves `have` false (and nothing listed) when there is none or neither header is valid.
pub fn load<N: Nor>(nor: &mut N, st: &mut Stations) {
    *st = Stations::NEW;
    let mut gens = [None, None];
    let mut h = [0u8; REC];
    for (half, g) in gens.iter_mut().enumerate() {
        nor.read(TBL_ADDR + half as u32 * SECTOR, &mut h);
        *g = header_gen(&h);
    }
    st.half = match gens {
        [None, None] => return,
        [Some(_), None] => 0,
        [None, Some(_)] => 1,
        [Some(a), Some(b)] => (a.wrapping_sub(b) as i16 >= 0) as u8 ^ 1,
    };
    st.gen = gens[st.half as usize].unwrap_or(0);
    st.have = true;
    for rec in 1..=SLOTS {
        nor.read(st.rec_addr(rec), &mut h);
        match parse_rec(&h) {
            Rec::Free => {}
            Rec::Dead => st.used = rec,
            Rec::Live(f) => {
                st.fq[st.n] = f;
                st.slot[st.n] = rec as u8;
                st.n += 1;
                st.used = rec;
            }
        }
    }
}

/// Copies the live records into the other half and makes it the table (or, with no table yet, writes the core's
/// bare list as half 0). The old half is left alone until the next rebuild erases it.
pub fn rebuild<N: Nor>(nor: &mut N, st: &mut Stations) -> bool {
    let target = if st.have { st.half ^ 1 } else { 0 };
    let gen = if st.have { st.gen.wrapping_add(1) } else { 1 };
    if !tbl_erase(nor, target as u32) {
        return false;
    }
    let base = TBL_ADDR + target as u32 * SECTOR;
    let mut slot = [0u8; SLOTS];
    for i in 0..st.n {
        let r = read_rec(nor, st, i);
        if !tbl_write(nor, base + ((i + 1) * REC) as u32, &r) {
            return false;
        }
        slot[i] = (i + 1) as u8;
    }
    if !tbl_write(nor, base, &make_header(gen)) {
        return false;
    }
    st.slot = slot;
    st.half = target;
    st.gen = gen;
    st.have = true;
    st.used = st.n;
    true
}

/// Adds a station without a name. The caller makes sure there is room (`st.used < SLOTS`) and a table.
pub fn append<N: Nor>(nor: &mut N, st: &mut Stations, freq: u16) -> bool {
    if !st.have || st.used >= SLOTS || st.n >= SLOTS || !(LO..=HI).contains(&freq) {
        return false;
    }
    if !tbl_write(nor, st.rec_addr(st.used + 1), &make_rec(freq, &[], &[])) {
        return false;
    }
    st.used += 1;
    st.fq[st.n] = freq;
    st.slot[st.n] = st.used as u8;
    st.n += 1;
    true
}

/// Clears station `i`'s frequency (a flash write can only clear bits, so the record stays as a dead one).
pub fn delete<N: Nor>(nor: &mut N, st: &mut Stations, i: usize) -> bool {
    if !st.have || i >= st.n || !tbl_write(nor, st.rec_addr(st.slot[i] as usize), &[0, 0]) {
        return false;
    }
    for j in i..st.n - 1 {
        st.fq[j] = st.fq[j + 1];
        st.slot[j] = st.slot[j + 1];
    }
    st.n -= 1;
    true
}

// ---- pure helpers --------------------------------------------------------------------------------------------

/// Length of the text in a padded field: up to the first 0x00 or 0xFF, without trailing spaces.
pub fn text_len(f: &[u8]) -> usize {
    let mut n = f.iter().position(|&b| b == 0 || b == 0xFF).unwrap_or(f.len());
    while n > 0 && f[n - 1] == b' ' {
        n -= 1;
    }
    n
}

/// Copies a field's text, printable ASCII only. Returns the length.
fn copy_text(dst: &mut [u8], src: &[u8]) -> usize {
    let n = text_len(src).min(dst.len());
    for i in 0..n {
        dst[i] = if (0x20..0x7F).contains(&src[i]) { src[i] } else { b'?' };
    }
    n
}

pub fn fmt_deci(deci: u16, out: &mut [u8; 6]) -> usize {
    let mut n = 0;
    if deci >= 1000 {
        out[n] = b'0' + (deci / 1000) as u8;
        n += 1;
    }
    out[n] = b'0' + (deci / 100 % 10) as u8;
    out[n + 1] = b'0' + (deci / 10 % 10) as u8;
    out[n + 2] = b'.';
    out[n + 3] = b'0' + (deci % 10) as u8;
    n + 4
}

pub fn step(cur: u16, up: bool) -> u16 {
    if up {
        if cur >= HI { LO } else { cur + 1 }
    } else if cur <= LO {
        HI
    } else {
        cur - 1
    }
}

/// The next saved frequency above (or below) `cur`, wrapping round the band; None when no other frequency is saved.
pub fn neighbour(fq: &[u16], cur: u16, up: bool) -> Option<u16> {
    let band = (HI - LO + 1) as i32;
    let dist = |f: u16| if up { f as i32 - cur as i32 } else { cur as i32 - f as i32 }.rem_euclid(band);
    fq.iter().copied().filter(|&f| f != cur).min_by_key(|&f| dist(f))
}

/// What typing a digit does: the frequency to tune to, if the digits typed so far are complete. Four digits always
/// are; three are when a fourth could not make a valid frequency (981 = 98.1, but 100 waits for 1005).
pub fn typed(digits: &[u8]) -> Option<u16> {
    let v = digits.iter().fold(0u16, |a, &d| a * 10 + d as u16);
    match digits.len() {
        4 => Some(v),
        3 if v >= LO => Some(v),
        _ => None,
    }
}

/// How many of the four signal bars to light for the tuner's 0-255 RSSI reading.
pub fn bars(rssi: u8) -> u8 {
    [12u8, 24, 40, 56].iter().filter(|&&t| rssi >= t).count() as u8
}

const SCALE_X: i16 = 8;
const SCALE_W: i32 = 144;

/// x of a frequency on the tuning scale.
pub fn scale_x(deci: u16) -> i16 {
    SCALE_X + ((deci.clamp(LO, HI) - LO) as i32 * SCALE_W / (HI - LO) as i32) as i16
}

// ---- the app ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
struct Snap {
    deci: u16,
    stn: u8,
    n: u8,
    seeking: bool,
    bars: u8,
    ilen: u8,
    idig: [u8; 4],
}

impl Snap {
    const NEW: Snap = Snap { deci: 0, stn: 0, n: 0, seeking: false, bars: 0, ilen: 0, idig: [0; 4] };
}

struct App {
    st: Stations,
    deci: u16,
    list: bool,
    seek_left: u32,
    rssi: u8,
    ilen: u8,
    idig: [u8; 4],
    sel: u16,
    armed: bool,
    toast: u8,
    toast_left: u32,
    toast_up: bool,
    snap: Snap,
    snap_ok: bool,
}

static mut APP: App = App {
    st: Stations::NEW,
    deci: 0,
    list: false,
    seek_left: 0,
    rssi: 0,
    ilen: 0,
    idig: [0; 4],
    sel: 0,
    armed: false,
    toast: 0,
    toast_left: 0,
    toast_up: false,
    snap: Snap::NEW,
    snap_ok: false,
};

fn app() -> &'static mut App {
    unsafe { &mut *core::ptr::addr_of_mut!(APP) }
}

struct ApiNor<'a>(&'a Api);

impl Nor for ApiNor<'_> {
    fn read(&mut self, addr: u32, out: &mut [u8]) {
        (self.0.nor_read)(addr, out.as_mut_ptr(), out.len() as u32);
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> bool {
        (self.0.nor_write)(addr, data.as_ptr(), data.len() as u32)
    }
    fn erase(&mut self, addr: u32) -> bool {
        (self.0.nor_erase_sector)(addr)
    }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    let a = app();
    match ev {
        AppEvent::Enter => enter(api, a),
        AppEvent::Key { id, kind } => return key(api, a, id, kind),
        AppEvent::Tick { dt_100us } => tick(api, a, dt_100us),
        AppEvent::Draw => draw(api, a),
        AppEvent::Leave => {}
    }
    AppResult::Continue
}

fn enter(api: &Api, a: &mut App) {
    // A broadcast frequency typed on the main screen arrives here as two bytes (tenths of a MHz).
    let mut want = [0u8; 2];
    let start = match (api.handoff_read)(want.as_mut_ptr(), 2) {
        2 if (LO..=HI).contains(&u16::from_le_bytes(want)) => u16::from_le_bytes(want),
        _ => DEFAULT,
    };
    (api.fm_tune_khz)(start as u32 * 100);
    a.st = Stations::NEW;
    a.deci = start;
    a.list = false;
    a.seek_left = 0;
    a.rssi = 0;
    a.ilen = 0;
    a.armed = false;
    a.toast = 0;
    a.toast_up = false;
    a.snap_ok = false;
    load(&mut ApiNor(api), &mut a.st);
    if !a.st.have {
        // no table: the core's bare list (65535 = empty slot)
        let mut raw = [0xFFu8; CORE_LIST * 2];
        if (api.fm_channels_load)(raw.as_mut_ptr(), raw.len() as u16) {
            for p in raw.chunks_exact(2) {
                let f = u16::from_le_bytes([p[0], p[1]]);
                if (LO..=HI).contains(&f) && a.st.n < SLOTS {
                    a.st.fq[a.st.n] = f;
                    a.st.n += 1;
                }
            }
        }
    }
    (api.set_speaker)(true);
}

fn fail(api: &Api) -> ! {
    (api.app_fault)(TBL_ADDR)
}

fn tune(api: &Api, a: &mut App, deci: u16) {
    a.deci = deci;
    (api.fm_tune_khz)(deci as u32 * 100);
}

fn say(a: &mut App, msg: u8) {
    a.toast = msg;
    a.toast_left = TOAST_100US;
}

fn key(api: &Api, a: &mut App, id: u8, kind: u8) -> AppResult {
    let press = kind == KIND_SINGLE || kind == KIND_REPEAT;
    a.toast = 0;
    if a.seek_left > 0 {
        // the tuner is busy: only EXIT is heard
        return exit_if(api, id == KEY_EXIT && (kind == KIND_SINGLE || kind == KIND_LONG));
    }
    if a.list {
        list_key(api, a, id, kind);
        return AppResult::Continue;
    }
    match (kind, id) {
        (KIND_SINGLE | KIND_LONG, KEY_EXIT) => {
            if a.ilen > 0 {
                a.ilen = 0;
            } else {
                return exit_if(api, true);
            }
        }
        (KIND_SINGLE, 0..=9) => digit(api, a, id),
        (_, KEY_UP) | (_, KEY_DOWN) if press => {
            a.ilen = 0;
            let d = step(a.deci, id == KEY_UP);
            tune(api, a, d);
        }
        (KIND_LONG, KEY_UP) | (KIND_LONG, KEY_DOWN) => {
            a.ilen = 0;
            (api.set_speaker)(false);
            (api.fm_seek)(id == KEY_UP);
            a.seek_left = SEEK_TIMEOUT_100US;
        }
        (KIND_SINGLE, KEY_STAR) | (KIND_SINGLE, KEY_POUND) => {
            a.ilen = 0;
            match neighbour(&a.st.fq[..a.st.n], a.deci, id == KEY_POUND) {
                Some(f) => tune(api, a, f),
                None => say(a, 4),
            }
        }
        (KIND_SINGLE, KEY_MENU) => open_list(api, a),
        _ => {}
    }
    AppResult::Continue
}

fn exit_if(api: &Api, yes: bool) -> AppResult {
    if yes {
        (api.fm_power_off)();
        AppResult::Exit
    } else {
        AppResult::Continue
    }
}

fn digit(api: &Api, a: &mut App, d: u8) {
    a.idig[a.ilen as usize] = d;
    a.ilen += 1;
    if let Some(f) = typed(&a.idig[..a.ilen as usize]) {
        a.ilen = 0;
        if (LO..=HI).contains(&f) {
            tune(api, a, f);
        } else {
            say(a, 5);
        }
    }
}

/// Number of rows above the stations: the "save this" row, present while the tuned frequency is not saved.
fn lead(a: &App) -> usize {
    a.st.find(a.deci).is_none() as usize
}

fn open_list(api: &Api, a: &mut App) {
    a.list = true;
    a.armed = false;
    a.ilen = 0;
    a.sel = match a.st.find(a.deci) {
        Some(i) => i as u16,
        None => 0,
    };
    (api.toast)(core::ptr::null(), 0); // the shared list repaints in full over whatever was on the screen
    a.toast_up = false;
}

fn list_key(api: &Api, a: &mut App, id: u8, kind: u8) {
    let total = a.st.n + lead(a);
    let first = lead(a);
    let sel = a.sel as usize;
    match (kind, id) {
        (KIND_SINGLE | KIND_REPEAT, KEY_UP) => a.sel = ((sel + total - 1) % total) as u16,
        (KIND_SINGLE | KIND_REPEAT, KEY_DOWN) => a.sel = ((sel + 1) % total) as u16,
        (KIND_SINGLE, KEY_STAR) if sel >= first => a.armed = true,
        (KIND_SINGLE, KEY_MENU) => {
            if a.armed {
                a.armed = false;
                delete_selected(api, a, sel - first);
            } else if sel < first {
                save_current(api, a);
            } else {
                let f = a.st.fq[sel - first];
                tune(api, a, f);
                a.list = false;
                a.snap_ok = false;
            }
            return;
        }
        (KIND_SINGLE | KIND_LONG, KEY_EXIT) => {
            if !a.armed {
                a.list = false;
                a.snap_ok = false;
            }
        }
        _ => return,
    }
    if id != KEY_STAR {
        a.armed = false;
    }
}

fn save_current(api: &Api, a: &mut App) {
    if a.st.n >= SLOTS {
        return say(a, 3);
    }
    let mut nor = ApiNor(api);
    if (!a.st.have || a.st.used >= SLOTS) && !rebuild(&mut nor, &mut a.st) {
        fail(api);
    }
    if !append(&mut nor, &mut a.st, a.deci) {
        fail(api);
    }
    a.sel = (a.st.n - 1) as u16;
    say(a, 1);
}

fn delete_selected(api: &Api, a: &mut App, i: usize) {
    let mut nor = ApiNor(api);
    if !a.st.have && !rebuild(&mut nor, &mut a.st) {
        fail(api);
    }
    if !delete(&mut nor, &mut a.st, i) {
        fail(api);
    }
    let i = i.min(a.st.n.saturating_sub(1));
    a.sel = (i + lead(a)) as u16;
    say(a, 2);
}

fn tick(api: &Api, a: &mut App, dt: u32) {
    let status = (api.fm_status)();
    a.rssi = (status >> 8) as u8;
    a.toast_left = a.toast_left.saturating_sub(dt);
    if a.toast_left == 0 {
        a.toast = 0;
    }
    if a.seek_left > 0 {
        let left = a.seek_left.saturating_sub(dt);
        let done = status & 1 != 0;
        let failed = status & 2 != 0;
        if !done && !failed && left > 0 {
            a.seek_left = left;
        } else {
            if !failed {
                a.deci = ((((api.fm_tuned_freq_khz)() + 50) / 100) as u16).clamp(LO, HI);
            }
            a.seek_left = 0;
            (api.set_speaker)(true);
        }
    }
}

// ---- drawing ------------------------------------------------------------------------------------------------

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GHOST: u16 = 0x18C3;
const GREY: u16 = 0x9492;
const DIM: u16 = 0x5AEB;
const AMBER: u16 = 0xFD40;
const STATION: u16 = 0x4DFF;
const TITLE_BG: u16 = 0xDEDB;
const TITLE_FG: u16 = 0x0000;
const BAR_OFF: u16 = 0xA514;
const HINT_BG: u16 = 0x2124;
const HINT_FG: u16 = 0xA514;

const F5: u8 = 0;
const F6: u8 = 1;
const F9: u8 = 2;
const W: i16 = 160;

const TITLE_H: i16 = 14;
const DIG_Y: i16 = 18;
const NAME_Y: i16 = 54;
const DETAIL_Y: i16 = 74;
const MARK_Y: i16 = 84; // the marker for the tuned frequency, 7 rows
const STN_Y: i16 = 91; // saved-station marks
const BASE_Y: i16 = 97;
const LABEL_Y: i16 = 104;
const HINT_Y: i16 = 112;

const VIS: usize = 7;
const LIST_TOP: i16 = 14;
const ROW_H: i16 = 14;

fn fill(api: &Api, x: i16, y: i16, w: i16, h: i16, c: u16) {
    (api.fill_rect)(x, y, w as u16, h as u16, c);
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font);
}

fn text_c(api: &Api, y: i16, s: &[u8], fg: u16, font: u8) {
    let cw = [5, 6, 9][font as usize];
    text(api, (W - s.len() as i16 * cw) / 2, y, s, fg, BLACK, font);
}

fn toast_text(msg: u8) -> &'static [u8] {
    match msg {
        1 => b"SAVED",
        2 => b"DELETED",
        3 => b"LIST FULL",
        4 => b"NO SAVED STATIONS",
        _ => b"RANGE 87.5-108.0",
    }
}

fn draw(api: &Api, a: &mut App) {
    if a.toast != 0 {
        a.toast_up = true;
    } else if a.toast_up {
        a.toast_up = false;
        (api.toast)(core::ptr::null(), 0);
        a.snap_ok = false;
    }
    if a.list {
        draw_list(api, a);
    } else {
        draw_tuner(api, a);
    }
    if a.toast != 0 {
        let t = toast_text(a.toast);
        (api.toast)(t.as_ptr(), t.len() as u16);
    }
}

// 7-segment glyphs: bit 0..6 = a (top), b (top right), c (bottom right), d (bottom), e (bottom left), f (top left), g (middle)
const SEGS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
// x, y, w, h of each segment in an 18 x 32 digit
const SEG_RECT: [[i16; 4]; 7] = [[3, 0, 12, 3], [15, 2, 3, 12], [15, 17, 3, 12], [3, 29, 12, 3], [0, 17, 3, 12], [0, 2, 3, 12], [3, 14, 12, 3]];
const DASH: u8 = 10;
const DOT: u8 = 11;

fn glyph_w(g: u8) -> i16 {
    if g == DOT { 8 } else { 22 }
}

/// Large digits across the middle of the screen, centred; `g` holds digits 0-9, DASH or DOT.
fn draw_digits(api: &Api, g: &[u8], on: u16) {
    fill(api, 20, DIG_Y, W - 40, 32, BLACK);
    let total = g.iter().map(|&c| glyph_w(c)).sum::<i16>() - 4;
    let mut x = (W - total) / 2;
    for &c in g {
        if c == DOT {
            fill(api, x, DIG_Y + 28, 4, 4, on);
        } else {
            let m = if c == DASH { 0x40 } else { SEGS[c as usize] };
            for (i, r) in SEG_RECT.iter().enumerate() {
                fill(api, x + r[0], DIG_Y + r[1], r[2], r[3], if m >> i & 1 != 0 { on } else { GHOST });
            }
        }
        x += glyph_w(c);
    }
}

/// A step arrow: 5 wide, 9 tall.
fn arrow(api: &Api, x: i16, left: bool) {
    for i in 0..9i16 {
        let d = (i - 4).abs();
        fill(api, if left { x + d } else { x }, DIG_Y + 11 + i, 5 - d, 1, GREY);
    }
}

fn draw_title(api: &Api, bars: u8) {
    fill(api, 0, 0, W, TITLE_H, TITLE_BG);
    text(api, (W - 8 * 6) / 2, 2, b"FM RADIO", TITLE_FG, TITLE_BG, F6);
    for i in 0..4i16 {
        let h = 3 + 2 * i;
        fill(api, 141 + 4 * i, 12 - h, 3, h, if (i as u8) < bars { TITLE_FG } else { BAR_OFF });
    }
}

fn draw_hint(api: &Api, s: &[u8]) {
    fill(api, 0, HINT_Y, W, 16, HINT_BG);
    text(api, 4, HINT_Y + 3, s, HINT_FG, HINT_BG, F6);
}

fn draw_tuner(api: &Api, a: &mut App) {
    let mut s = Snap {
        deci: a.deci,
        stn: a.st.find(a.deci).map_or(255, |i| i as u8),
        n: a.st.n as u8,
        seeking: a.seek_left > 0,
        bars: bars(a.rssi),
        ilen: a.ilen,
        idig: a.idig,
    };
    for i in a.ilen as usize..4 {
        s.idig[i] = 0;
    }
    let p = a.snap;
    let full = !a.snap_ok;
    if full {
        fill(api, 0, 0, W, 128, BLACK);
        draw_title(api, s.bars);
        arrow(api, 6, true);
        arrow(api, 149, false);
    } else if p.bars != s.bars {
        draw_title(api, s.bars);
    }
    if full || (p.deci, p.ilen, p.idig) != (s.deci, s.ilen, s.idig) {
        let mut g = [0u8; 6];
        let n = if s.ilen > 0 {
            for i in 0..5 {
                g[i] = match i {
                    3 => DOT,
                    _ if i < 3 && i < s.ilen as usize => s.idig[i],
                    4 if s.ilen == 4 => s.idig[3],
                    _ => DASH,
                };
            }
            5
        } else {
            let mut t = [0u8; 6];
            let n = fmt_deci(s.deci, &mut t);
            for i in 0..n {
                g[i] = if t[i] == b'.' { DOT } else { t[i] - b'0' };
            }
            n
        };
        draw_digits(api, &g[..n], if s.ilen > 0 { AMBER } else { WHITE });
    }
    if full || (p.deci, p.stn, p.n, p.seeking) != (s.deci, s.stn, s.n, s.seeking) || (p.ilen > 0) != (s.ilen > 0) {
        draw_name(api, a, &s);
    }
    if full || (p.ilen > 0) != (s.ilen > 0) {
        draw_hint(api, if s.ilen > 0 { b"0-9 type  EXIT clear" } else { b"UP/DN step  MENU stations" });
    }
    if full || p.n != s.n || p.stn != s.stn {
        draw_scale(api, a);
    }
    if full || p.n != s.n || p.deci != s.deci {
        draw_marker(api, s.deci);
    }
    a.snap = s;
    a.snap_ok = true;
}

fn draw_name(api: &Api, a: &App, s: &Snap) {
    fill(api, 0, NAME_Y - 2, W, 32, BLACK);
    if s.seeking {
        text_c(api, NAME_Y, b"SEEKING", AMBER, F9);
    } else if s.ilen > 0 {
        return;
    } else if s.stn == 255 {
        text_c(api, DETAIL_Y, b"Not saved", GREY, F6);
    } else {
        let r = read_rec(&mut ApiNor(api), &a.st, s.stn as usize);
        let mut b = [0u8; 16];
        let n = copy_text(&mut b[..NAME_LEN], &r[2..14]);
        if n > 0 {
            text_c(api, NAME_Y, &b[..n], WHITE, F9);
        }
        let n = copy_text(&mut b, &r[14..30]);
        text_c(api, DETAIL_Y, if n > 0 { &b[..n] } else { b"Saved" }, GREY, F6);
    }
}

fn draw_scale(api: &Api, a: &App) {
    fill(api, 0, STN_Y, W, 21, BLACK);
    fill(api, SCALE_X, BASE_Y, SCALE_W as i16 + 1, 1, DIM);
    for mhz in 88..=108u16 {
        let x = scale_x(mhz * 10);
        let major = mhz % 5 == 0;
        fill(api, x, BASE_Y + 1, 1, if major { 5 } else { 3 }, if major { GREY } else { DIM });
        if major {
            let mut t = [0u8; 6];
            let n = fmt_deci(mhz * 10, &mut t) - 2;
            text(api, x - n as i16 * 5 / 2, LABEL_Y, &t[..n], GREY, BLACK, F5);
        }
    }
    for &f in &a.st.fq[..a.st.n] {
        fill(api, scale_x(f), STN_Y, 2, 6, STATION);
    }
}

fn draw_marker(api: &Api, deci: u16) {
    fill(api, 0, MARK_Y, W, 7, BLACK);
    let x = scale_x(deci);
    for i in 0..5i16 {
        fill(api, x - (4 - i), MARK_Y + 2 + i, 9 - 2 * i, 1, WHITE);
    }
}

fn draw_list(api: &Api, a: &mut App) {
    static mut ROWS: [ListRow; VIS] = [ListRow { label: [0; 16], value: [0; 18], has_value: false, cursor: 0 }; VIS];
    let lead = lead(a);
    let total = a.st.n + lead;
    let sel = a.sel as usize;
    let top = sel.saturating_sub(VIS / 2).min(total.saturating_sub(VIS));
    let rows = unsafe { &mut *core::ptr::addr_of_mut!(ROWS) };
    let mut nor = ApiNor(api);
    let mut playing = None;
    for (slot, row) in rows.iter_mut().enumerate() {
        let g = top + slot;
        *row = ListRow { label: [0; 16], value: [0; 18], has_value: false, cursor: -1 };
        if g >= total {
            continue;
        }
        let mut t = [0u8; 6];
        let f = if g < lead {
            row.label[..11].copy_from_slice(b"+ SAVE THIS");
            a.deci
        } else {
            let i = g - lead;
            let r = read_rec(&mut nor, &a.st, i);
            row.label[..2].copy_from_slice(b"  ");
            let n = copy_text(&mut row.label[2..14], &r[2..14]);
            if n == 0 {
                row.label[2..11].copy_from_slice(b"(unnamed)");
            }
            if a.st.fq[i] == a.deci {
                playing = Some(slot as i16);
            }
            a.st.fq[i]
        };
        let n = fmt_deci(f, &mut t);
        row.value[..n].copy_from_slice(&t[..n]);
        row.has_value = true;
        if g == sel && a.armed {
            row.cursor = bf5rh_abi::ROW_ARMED;
        }
    }
    let hint: &[u8] = match (a.armed, sel < lead) {
        (true, _) => b"MENU confirm  EXIT cancel",
        (_, true) => b"MENU save  EXIT back",
        _ => b"MENU play   * delete",
    };
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);
    (api.draw_list)(b"STATIONS".as_ptr(), 8, rows.as_ptr(), VIS as u16, top as u16, a.sel, total as u16, false);
    if let Some(slot) = playing {
        // a speaker beside the station that is playing
        let y = LIST_TOP + slot * ROW_H + 3;
        let c = if a.armed && top + slot as usize == sel { 0 } else { WHITE };
        fill(api, 4, y + 2, 2, 4, c);
        fill(api, 6, y + 1, 1, 6, c);
        fill(api, 7, y, 1, 8, c);
        fill(api, 9, y + 2, 1, 4, c);
        fill(api, 11, y + 1, 1, 6, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../../../tests/fixtures/fm-stations.bin");

    /// A 2 MB flash with real NOR behaviour (writes only clear bits, an erase sets a whole 4 KB sector) that records
    /// everything done to it. `die_after` cuts the power after that many writes.
    struct Mem {
        mem: Vec<u8>,
        writes: Vec<(u32, usize)>,
        erases: Vec<u32>,
        die_after: Option<usize>,
    }

    impl Mem {
        fn new() -> Mem {
            Mem { mem: vec![0xFF; 0x20_0000], writes: vec![], erases: vec![], die_after: None }
        }
        fn with(table: &[u8]) -> Mem {
            let mut m = Mem::new();
            m.mem[TBL_ADDR as usize..TBL_ADDR as usize + table.len()].copy_from_slice(table);
            m
        }
    }

    impl Nor for Mem {
        fn read(&mut self, addr: u32, out: &mut [u8]) {
            out.copy_from_slice(&self.mem[addr as usize..addr as usize + out.len()]);
        }
        fn write(&mut self, addr: u32, data: &[u8]) -> bool {
            if self.die_after == Some(self.writes.len()) {
                return false;
            }
            assert_eq!(addr / 256, (addr + data.len() as u32 - 1) / 256, "a write must stay in one flash page");
            self.writes.push((addr, data.len()));
            for (i, &b) in data.iter().enumerate() {
                self.mem[addr as usize + i] &= b;
            }
            true
        }
        fn erase(&mut self, addr: u32) -> bool {
            assert_eq!(addr % SECTOR, 0);
            self.erases.push(addr);
            self.mem[addr as usize..(addr + SECTOR) as usize].fill(0xFF);
            true
        }
    }

    fn inside(m: &Mem) {
        for &(a, l) in &m.writes {
            assert!(a >= TBL_ADDR && a + l as u32 <= TBL_ADDR + TBL_BYTES, "write outside the table: {a:#x}+{l}");
        }
        for &a in &m.erases {
            assert!(a >= TBL_ADDR && a + SECTOR <= TBL_ADDR + TBL_BYTES, "erase outside the table: {a:#x}");
        }
    }

    fn loaded(m: &mut Mem) -> Stations {
        let mut st = Stations::NEW;
        load(m, &mut st);
        st
    }

    fn name_of(m: &mut Mem, st: &Stations, i: usize) -> String {
        let r = read_rec(m, st, i);
        String::from_utf8(r[2..2 + text_len(&r[2..14])].to_vec()).unwrap()
    }

    #[test]
    fn the_table_is_clear_of_everything_else_in_the_flash() {
        // FSK PROBE's capture ends at 0x1D0000; the channel extras table starts at 0x1E0000
        assert!(TBL_ADDR >= 0x1D_0000);
        assert!(TBL_ADDR + TBL_BYTES <= 0x1E_0000);
        assert_eq!(TBL_ADDR % SECTOR, 0);
        assert_eq!(TBL_BYTES, 2 * SECTOR);
        assert_eq!(SLOTS * REC + REC, SECTOR as usize);
    }

    #[test]
    fn range_check_refuses_anything_outside_the_table() {
        assert!(in_table(TBL_ADDR, 32));
        assert!(in_table(TBL_ADDR + TBL_BYTES - 2, 2));
        assert!(in_table(TBL_ADDR, TBL_BYTES));
        assert!(!in_table(TBL_ADDR - 1, 1));
        assert!(!in_table(TBL_ADDR + TBL_BYTES - 1, 2));
        assert!(!in_table(TBL_ADDR, TBL_BYTES + 1));
        assert!(!in_table(0x1E_0000, 20));
        assert!(!in_table(0x1D_0000, 32));
        assert!(!in_table(0xC000, 60), "the core's own bare list is not ours to write");
        assert!(!in_table(u32::MAX, 2));
    }

    #[test]
    fn writes_and_erases_outside_the_table_are_refused_before_reaching_the_flash() {
        let mut m = Mem::new();
        for addr in [0x1D_7FE0u32, TBL_ADDR + TBL_BYTES - 16, TBL_ADDR + TBL_BYTES, 0x1E_0000, 0xC000, 0x14_C000, u32::MAX - 3] {
            assert!(!tbl_write(&mut m, addr, &[0u8; 32]), "{addr:#x}");
        }
        assert!(!tbl_write(&mut m, TBL_ADDR, &[0u8; 8193]));
        assert!(!tbl_erase(&mut m, 2));
        assert!(!tbl_erase(&mut m, u32::MAX));
        assert!(m.writes.is_empty() && m.erases.is_empty());
        assert!(tbl_write(&mut m, TBL_ADDR + 32, &[0u8; 32]) && tbl_erase(&mut m, 1));
    }

    #[test]
    fn the_tools_table_reads_back() {
        // fm-stations.bin is what `fm_stations.py build` makes from firmware/tests/fixtures/fm-stations.csv
        let mut m = Mem::with(FIXTURE);
        let st = loaded(&mut m);
        assert!(st.have);
        assert_eq!(st.n, 4);
        assert_eq!(&st.fq[..4], &[901, 981, 1011, 1045]);
        assert_eq!(&st.slot[..4], &[1, 2, 3, 4]);
        assert_eq!(st.used, 4);
        assert_eq!(name_of(&mut m, &st, 0), "KERA");
        assert_eq!(name_of(&mut m, &st, 3), "THE POINT 45");
        let r = read_rec(&mut m, &st, 0);
        assert_eq!(&r[14..14 + text_len(&r[14..30])], b"Public Radio");
        assert_eq!(&read_rec(&mut m, &st, 1)[14..30], &[0xFF; 16], "no detail");
    }

    #[test]
    fn no_table_or_a_damaged_one_lists_nothing_and_changes_nothing() {
        let mut m = Mem::new();
        assert!(!loaded(&mut m).have);
        let mut bad = FIXTURE.to_vec();
        bad[0] = b'X';
        assert!(!loaded(&mut Mem::with(&bad)).have, "wrong magic");
        let mut bad = FIXTURE.to_vec();
        bad[4] = 2;
        assert!(!loaded(&mut Mem::with(&bad)).have, "another version");
        let mut bad = FIXTURE.to_vec();
        bad[REC - 1] = 0;
        assert!(!loaded(&mut Mem::with(&bad)).have, "header mark missing");
        let mut m = Mem::with(&[0u8; 8192]);
        assert!(!loaded(&mut m).have, "zeros");
        assert!(m.writes.is_empty() && m.erases.is_empty(), "loading never writes");
    }

    #[test]
    fn damaged_records_are_skipped_not_trusted() {
        let mut t = FIXTURE.to_vec();
        t[2 * REC] = 0x00; // record 2: frequency cleared, as a delete does
        t[2 * REC + 1] = 0x00;
        t[3 * REC + 31] = 0xFF; // record 3 loses its mark
        t[4 * REC] = 0x10; // record 4: 0x0010 = 16, not an FM frequency
        t[4 * REC + 1] = 0x00;
        let mut m = Mem::with(&t);
        let st = loaded(&mut m);
        assert_eq!(st.n, 1);
        assert_eq!(st.fq[0], 901);
        assert_eq!(st.used, 4, "dead records are not reused: the next free one is after them");
    }

    #[test]
    fn save_and_delete_only_append_or_clear_bits_and_survive_a_reload() {
        let mut m = Mem::with(FIXTURE);
        let mut st = loaded(&mut m);
        assert!(append(&mut m, &mut st, 1023));
        assert!(delete(&mut m, &mut st, 1));
        assert_eq!(&st.fq[..st.n], &[901, 1011, 1045, 1023]);
        assert!(m.erases.is_empty(), "no erase is needed to save or delete");
        inside(&m);
        let again = loaded(&mut m);
        assert_eq!(&again.fq[..again.n], &st.fq[..st.n]);
        assert_eq!(&again.slot[..again.n], &st.slot[..st.n]);
        assert_eq!(again.used, 5);
        assert_eq!(name_of(&mut m, &again, 1), "THE EDGE");
        assert_eq!(name_of(&mut m, &again, 3), "", "a saved frequency has no name");
        // refuses what it cannot represent
        assert!(!append(&mut m, &mut st, 874) && !append(&mut m, &mut st, 1081));
        assert!(!delete(&mut m, &mut st, 99));
    }

    #[test]
    fn a_bare_list_moves_into_a_table_on_the_first_change() {
        let mut m = Mem::new();
        let mut st = Stations::NEW;
        for f in [901u16, 981, 1045] {
            st.fq[st.n] = f;
            st.n += 1;
        }
        assert!(!st.have);
        assert!(rebuild(&mut m, &mut st));
        assert!(append(&mut m, &mut st, 1000));
        inside(&m);
        let now = loaded(&mut m);
        assert!(now.have);
        assert_eq!(&now.fq[..now.n], &[901, 981, 1045, 1000]);
        assert_eq!(m.erases, vec![TBL_ADDR], "half 0 only");
    }

    #[test]
    fn a_full_half_is_compacted_into_the_other_and_keeps_names() {
        let mut m = Mem::with(FIXTURE);
        let mut st = loaded(&mut m);
        let mut f = 880u16;
        while st.used < SLOTS {
            if st.find(f).is_none() {
                assert!(append(&mut m, &mut st, f));
            }
            f += 1;
            if f > HI {
                f = LO;
            }
        }
        assert!(!append(&mut m, &mut st, 1070), "no room left in the half");
        // delete most of them, leaving tombstones
        while st.n > 6 {
            let i = st.n - 1;
            assert!(delete(&mut m, &mut st, i));
        }
        assert_eq!(st.n, 6);
        let before: Vec<u16> = st.fq[..st.n].to_vec();
        assert!(rebuild(&mut m, &mut st));
        assert_eq!(m.erases, vec![TBL_ADDR + SECTOR], "the other half");
        assert_eq!(st.used, 6);
        assert_eq!(&st.slot[..6], &[1, 2, 3, 4, 5, 6]);
        inside(&m);
        let mut now = loaded(&mut m);
        assert_eq!(&now.fq[..now.n], &before[..]);
        assert_eq!(name_of(&mut m, &now, 0), "KERA");
        assert_eq!(name_of(&mut m, &now, 3), "THE POINT 45");
        assert!(append(&mut m, &mut now, 1070));
        // and back again
        assert!(rebuild(&mut m, &mut now));
        assert_eq!(*m.erases.last().unwrap(), TBL_ADDR);
        assert_eq!(loaded(&mut m).n, 7);
        inside(&m);
    }

    #[test]
    fn a_power_cut_at_any_point_of_a_rebuild_leaves_the_old_table() {
        let mut probe = Mem::with(FIXTURE);
        let mut st = loaded(&mut probe);
        assert!(delete(&mut probe, &mut st, 0));
        let start = probe.writes.len();
        assert!(rebuild(&mut probe, &mut st));
        let total = probe.writes.len() - start;
        assert_eq!(total, 3 + 1, "three records and the header");
        for cut in 0..total {
            let mut m = Mem::with(FIXTURE);
            let mut st = loaded(&mut m);
            assert!(delete(&mut m, &mut st, 0));
            m.die_after = Some(m.writes.len() + cut);
            assert!(!rebuild(&mut m, &mut st), "cut {cut}");
            m.die_after = None;
            let now = loaded(&mut m);
            assert_eq!(&now.fq[..now.n], &[981, 1011, 1045], "cut {cut}: still the old table, minus the deleted one");
            assert_eq!(now.half, 0);
        }
    }

    #[test]
    fn the_newer_header_wins_even_across_the_generation_wrap() {
        let mut t = vec![0xFFu8; 8192];
        t[..4096].copy_from_slice(&FIXTURE[..4096]);
        let mut other = FIXTURE[..4096].to_vec();
        other[REC] = 0xFF; // blank the first record's freq...
        other[REC + 1] = 0xFF;
        other[REC + 31] = 0xFF; // ...and its mark: one station fewer
        t[4096..].copy_from_slice(&other);
        for (g0, g1, want_half) in [(1u16, 2u16, 1u8), (2, 1, 0), (5, 5, 0), (0xFFFF, 0, 1), (0, 0xFFFF, 0)] {
            t[6..8].copy_from_slice(&g0.to_le_bytes());
            t[4096 + 6..4096 + 8].copy_from_slice(&g1.to_le_bytes());
            let st = loaded(&mut Mem::with(&t));
            assert_eq!(st.half, want_half, "{g0} vs {g1}");
            assert_eq!(st.n, if want_half == 1 { 3 } else { 4 });
        }
    }

    #[test]
    fn typed_frequencies_commit_as_soon_as_they_are_unambiguous() {
        assert_eq!(typed(&[9]), None);
        assert_eq!(typed(&[9, 8]), None);
        assert_eq!(typed(&[9, 8, 1]), Some(981));
        assert_eq!(typed(&[8, 7, 5]), Some(875));
        assert_eq!(typed(&[1, 0, 0]), None, "100 could be 100.x or 10.0x: wait for the fourth");
        assert_eq!(typed(&[1, 0, 1, 5]), Some(1015));
        assert_eq!(typed(&[0, 9, 8, 1]), Some(981));
        assert_eq!(typed(&[0, 0, 5]), None);
        assert_eq!(typed(&[1, 1, 5, 0]), Some(1150), "committed, then rejected as out of band by the caller");
    }

    #[test]
    fn stepping_and_jumping_wrap_round_the_band() {
        assert_eq!(step(1000, true), 1001);
        assert_eq!(step(HI, true), LO);
        assert_eq!(step(LO, false), HI);
        let s = [901, 981, 1045];
        assert_eq!(neighbour(&s, 981, true), Some(1045));
        assert_eq!(neighbour(&s, 1045, true), Some(901), "wraps");
        assert_eq!(neighbour(&s, 901, false), Some(1045));
        assert_eq!(neighbour(&s, 1000, false), Some(981), "from an unsaved frequency");
        assert_eq!(neighbour(&s, 1000, true), Some(1045));
        assert_eq!(neighbour(&[981], 981, true), None);
        assert_eq!(neighbour(&[], 981, true), None);
    }

    #[test]
    fn numbers_and_the_scale_come_out_right() {
        let mut b = [0u8; 6];
        let n = fmt_deci(981, &mut b);
        assert_eq!(&b[..n], b"98.1");
        let n = fmt_deci(1080, &mut b);
        assert_eq!(&b[..n], b"108.0");
        let n = fmt_deci(875, &mut b);
        assert_eq!(&b[..n], b"87.5");
        assert_eq!(scale_x(LO), 8);
        assert_eq!(scale_x(HI), 152);
        assert_eq!(scale_x(2000), 152);
        assert!(scale_x(900) < scale_x(950) && scale_x(950) < scale_x(1000));
        assert_eq!([bars(0), bars(11), bars(12), bars(40), bars(255)], [0, 0, 1, 3, 4]);
        assert_eq!(text_len(b"KERA\xff\xff\xff"), 4);
        assert_eq!(text_len(b"KERA  \0x"), 4);
        assert_eq!(text_len(b"\xff\xff"), 0);
        let mut d = [0u8; 6];
        assert_eq!(copy_text(&mut d, b"A\x01B\xff"), 3);
        assert_eq!(&d[..3], b"A?B");
    }
}
