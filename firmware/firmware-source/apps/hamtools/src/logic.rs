//! HAM TOOLS: station-ID timer, stopwatch, wavelength calculator and grid locator.
//!
//! Receive-only and integer-only. Never call `api.tone_on`: it maps to
//! `Radio::rtone_on`, which keys the transmitter, so the ID alert is visual
//! (flashing banner and backlight hold) and never audible.
//!
//! State lives in a `static`, which is reloaded with the app image, so leaving
//! the app loses the timers; Exit therefore has to be held while one is running.
//!
//! Keys: A/B = next tool, Exit = leave (hold if a timer runs).
//!   ID TIMER   Menu = "I just identified" (restart), * = arm/disarm, Up/Down = interval
//!   STOPWATCH  Menu = start/stop, * = lap, 0 = reset
//!   WAVELENGTH digits = frequency in kHz, * = clear
//!   GRID       shows the six-character Maidenhead locator of the stored position

use bf5rh_abi::{Api, AppEvent, AppResult, SETTING_COORD_NOT_SET};

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KEY_STAR: u8 = 10;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_AB: u8 = 17;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const TITLE_BG: u16 = 0x5800;
const CYAN: u16 = 0x07FF;
const YELLOW: u16 = 0xFFE0;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;

const TICKS_PER_S: u32 = 10_000; // uptime and Tick dt are in 100 us units
const ID_DEFAULT_S: u32 = 600; // FCC 97.119: every 10 minutes
const ID_MIN_S: u32 = 60;
const ID_MAX_S: u32 = 1800;
const ID_STEP_S: u32 = 60;
const SW_MAX: u32 = 5999 * 10 * 1000 + 9000; // 99:59.9 in 100 us units
const LAPS: usize = 4;
const FREQ_DIGITS: usize = 6;
const FREQ_DEFAULT_KHZ: u32 = 146_520;
/// c in m/s. Wavelength in mm is c * 1000 / f_hz, i.e. c / f_khz.
const SPEED_OF_LIGHT_M_S: u64 = 299_792_458;
const VELOCITY_FACTOR_PCT: u32 = 95;
const LONG_WAVE_MM: u32 = 100_000; // show metres from here up

// ---------------------------------------------------------------- pure logic

/// Wavelength in millimetres for a frequency in kHz (0 for 0 kHz).
pub fn wavelength_mm(khz: u32) -> u32 {
    if khz == 0 {
        return 0;
    }
    (SPEED_OF_LIGHT_M_S / khz as u64) as u32
}

/// Six-character Maidenhead locator from lat/lon in 1e-5 degrees.
pub fn grid_locator(lat_e5: i32, lon_e5: i32) -> [u8; 6] {
    let lon = (lon_e5.clamp(-17_999_999, 17_999_999) + 18_000_000) as u32;
    let lat = (lat_e5.clamp(-8_999_999, 8_999_999) + 9_000_000) as u32;
    let (lf, lr) = (lon / 2_000_000, lon % 2_000_000);
    let (af, ar) = (lat / 1_000_000, lat % 1_000_000);
    let (ls, lr2) = (lr / 200_000, lr % 200_000);
    let (as_, ar2) = (ar / 100_000, ar % 100_000);
    [
        b'A' + lf as u8,
        b'A' + af as u8,
        b'0' + ls as u8,
        b'0' + as_ as u8,
        b'a' + (lr2 * 24 / 200_000) as u8,
        b'a' + (ar2 * 24 / 100_000) as u8,
    ]
}

/// "MM:SS" into `out`, returns the length (minutes saturate at 99).
pub fn fmt_mmss(secs: u32, out: &mut [u8; 5]) -> usize {
    let m = (secs / 60).min(99);
    let s = secs % 60;
    out[0] = b'0' + (m / 10) as u8;
    out[1] = b'0' + (m % 10) as u8;
    out[2] = b':';
    out[3] = b'0' + (s / 10) as u8;
    out[4] = b'0' + (s % 10) as u8;
    5
}

/// Counts down to the next required station identification.
#[derive(Clone, Copy)]
pub struct IdTimer {
    pub interval_s: u32,
    pub armed: bool,
    pub elapsed: u32, // 100 us units since the last identification
}

impl IdTimer {
    pub const fn new() -> Self {
        IdTimer { interval_s: ID_DEFAULT_S, armed: false, elapsed: 0 }
    }
    pub fn advance(&mut self, dt: u32) {
        if self.armed {
            self.elapsed = self.elapsed.saturating_add(dt);
        }
    }
    pub fn due(&self) -> bool {
        self.armed && self.elapsed >= self.interval_s * TICKS_PER_S
    }
    /// Seconds left, or seconds overdue when `due()`.
    pub fn display_s(&self) -> u32 {
        let el = self.elapsed / TICKS_PER_S;
        if self.due() { el - self.interval_s } else { self.interval_s - el }
    }
    pub fn identified(&mut self) {
        self.elapsed = 0;
    }
    pub fn adjust(&mut self, up: bool) {
        if self.armed {
            return;
        }
        self.interval_s = if up {
            (self.interval_s + ID_STEP_S).min(ID_MAX_S)
        } else {
            self.interval_s.saturating_sub(ID_STEP_S).max(ID_MIN_S)
        };
    }
}

#[derive(Clone, Copy)]
pub struct Stopwatch {
    pub running: bool,
    pub elapsed: u32, // 100 us units
    pub laps: [u32; LAPS],
    pub lap_count: usize,
}

impl Stopwatch {
    pub const fn new() -> Self {
        Stopwatch { running: false, elapsed: 0, laps: [0; LAPS], lap_count: 0 }
    }
    pub fn advance(&mut self, dt: u32) {
        if self.running {
            self.elapsed = self.elapsed.saturating_add(dt).min(SW_MAX);
        }
    }
    pub fn lap(&mut self) {
        if !self.running {
            return;
        }
        if self.lap_count < LAPS {
            self.laps[self.lap_count] = self.elapsed;
            self.lap_count += 1;
        } else {
            self.laps.copy_within(1.., 0);
            self.laps[LAPS - 1] = self.elapsed;
        }
    }
    pub fn reset(&mut self) {
        *self = Stopwatch::new();
    }
}

/// Pushes a digit onto a kHz entry; ignores digits past `FREQ_DIGITS`.
pub fn push_digit(khz: u32, digit: u8, fresh: bool) -> u32 {
    let base = if fresh { 0 } else { khz };
    if base >= 10u32.pow(FREQ_DIGITS as u32 - 1) {
        return base;
    }
    base * 10 + digit as u32
}

// ------------------------------------------------------------------- state

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Id,
    Watch,
    Wave,
    Grid,
}

struct State {
    tool: Tool,
    id: IdTimer,
    sw: Stopwatch,
    khz: u32,
    fresh: bool,
    clock: u32, // free-running, drives the alert flash
    note: &'static [u8],
}

static mut STATE: State = State {
    tool: Tool::Id,
    id: IdTimer::new(),
    sw: Stopwatch::new(),
    khz: FREQ_DEFAULT_KHZ,
    fresh: true,
    clock: 0,
    note: b"",
};

fn next(t: Tool) -> Tool {
    match t {
        Tool::Id => Tool::Watch,
        Tool::Watch => Tool::Wave,
        Tool::Wave => Tool::Grid,
        Tool::Grid => Tool::Id,
    }
}

// ----------------------------------------------------------------- drawing

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) -> i16 {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font) as i16
}

/// Writes `v` as decimal at the end of `buf`, returns the filled slice start.
fn dec(buf: &mut [u8; 12], mut v: u32) -> usize {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            return i;
        }
    }
}

fn title(api: &Api, name: &[u8], bg: u16) {
    (api.fill_rect)(0, 0, 160, 14, bg);
    text(api, 6, 2, name, WHITE, bg, 1);
}

fn footer(api: &Api, s: &State, hint: &[u8]) {
    (api.draw_line)(0, 106, 160, 106, GRAY);
    if !s.note.is_empty() {
        text(api, 4, 110, s.note, YELLOW, BLACK, 0);
    }
    text(api, 4, 120, hint, GRAY, BLACK, 0);
}

fn draw_id(api: &Api, s: &State) {
    let due = s.id.due();
    let flash = due && (s.clock / (TICKS_PER_S / 2)) % 2 == 0;
    title(api, if due { b"ID NOW" } else { b"ID TIMER" }, if flash { RED } else { TITLE_BG });
    let mut t = [0u8; 5];
    let n = fmt_mmss(s.id.display_s(), &mut t);
    let colour = if due { RED } else if s.id.armed { GREEN } else { GRAY };
    text(api, 38, 34, if due { b"+" } else { b" " }, colour, BLACK, 2);
    text(api, 47, 34, &t[..n], colour, BLACK, 2);
    let mut b = [0u8; 12];
    let mut m = [0u8; 5];
    fmt_mmss(s.id.interval_s, &mut m);
    let i = dec(&mut b, 0);
    let _ = i;
    text(api, 4, 62, b"EVERY", GRAY, BLACK, 0);
    text(api, 40, 62, &m, WHITE, BLACK, 0);
    text(api, 4, 74, if s.id.armed { b"ARMED " } else { b"OFF   " }, colour, BLACK, 1);
    if due {
        text(api, 4, 90, b"SAY YOUR CALLSIGN", YELLOW, BLACK, 0);
    }
    footer(api, s, b"MENU:IDed *:ARM UP/DN:TIME");
}

fn draw_watch(api: &Api, s: &State) {
    title(api, b"STOPWATCH", TITLE_BG);
    let tenths = s.sw.elapsed / 1000;
    let mut t = [0u8; 5];
    let n = fmt_mmss(tenths / 10, &mut t);
    let x = text(api, 24, 22, &t[..n], if s.sw.running { GREEN } else { WHITE }, BLACK, 2);
    text(api, x, 22, b".", WHITE, BLACK, 2);
    text(api, x + 9, 22, &[b'0' + (tenths % 10) as u8], WHITE, BLACK, 2);
    for i in 0..s.sw.lap_count {
        let l = s.sw.laps[i] / 1000;
        let mut lt = [0u8; 5];
        let ln = fmt_mmss(l / 10, &mut lt);
        let y = 48 + 12 * i as i16;
        text(api, 6, y, &[b'1' + i as u8], GRAY, BLACK, 0);
        let nx = text(api, 18, y, &lt[..ln], CYAN, BLACK, 1);
        text(api, nx, y, &[b'.', b'0' + (l % 10) as u8], CYAN, BLACK, 1);
    }
    footer(api, s, b"MENU:GO/STOP *:LAP 0:RESET");
}

fn draw_wave(api: &Api, s: &State) {
    title(api, b"WAVELENGTH", TITLE_BG);
    let mhz = s.khz / 1000;
    let frac = s.khz % 1000;
    let mut b = [0u8; 12];
    let i = dec(&mut b, mhz);
    let x = text(api, 6, 20, &b[i..], YELLOW, BLACK, 2);
    let x = text(api, x, 20, b".", YELLOW, BLACK, 2);
    let f = [b'0' + (frac / 100) as u8, b'0' + (frac / 10 % 10) as u8, b'0' + (frac % 10) as u8];
    let x = text(api, x, 20, &f, YELLOW, BLACK, 2);
    text(api, x + 3, 26, b"MHz", GRAY, BLACK, 0);
    let lam = wavelength_mm(s.khz);
    rows(api, 46, b"FULL", lam);
    rows(api, 60, b"1/2", lam / 2);
    rows(api, 74, b"1/4", lam / 4);
    rows(api, 88, b"1/4 VF95", lam / 4 * VELOCITY_FACTOR_PCT / 100);
    footer(api, s, b"DIGITS:kHz  *:CLEAR");
}

/// One row: label, cm with one decimal, inches with one decimal. At 100 m and above (below
/// roughly 3 MHz) it switches to metres alone, so a long wavelength never runs off the screen.
fn rows(api: &Api, y: i16, label: &[u8], mm: u32) {
    text(api, 4, y, label, GRAY, BLACK, 0);
    let mut b = [0u8; 12];
    if mm >= LONG_WAVE_MM {
        let i = dec(&mut b, mm / 1000);
        let x = text(api, 50, y, &b[i..], WHITE, BLACK, 1);
        let x = text(api, x, y, &[b'.', b'0' + (mm / 100 % 10) as u8], WHITE, BLACK, 1);
        text(api, x, y, b"m", GRAY, BLACK, 0);
        return;
    }
    let i = dec(&mut b, mm / 10);
    let x = text(api, 50, y, &b[i..], WHITE, BLACK, 1);
    let x = text(api, x, y, &[b'.', b'0' + (mm % 10) as u8], WHITE, BLACK, 1);
    text(api, x, y, b"cm", GRAY, BLACK, 0);
    let inch_tenths = mm * 100 / 254;
    let mut c = [0u8; 12];
    let j = dec(&mut c, inch_tenths / 10);
    let x = text(api, 110, y, &c[j..], CYAN, BLACK, 1);
    text(api, x, y, &[b'.', b'0' + (inch_tenths % 10) as u8], CYAN, BLACK, 1);
}

fn draw_grid(api: &Api, s: &State) {
    title(api, b"GRID LOCATOR", TITLE_BG);
    let lat = (api.settings_get)(5) as i32;
    let lon = (api.settings_get)(6) as i32;
    if lat == SETTING_COORD_NOT_SET || lon == SETTING_COORD_NOT_SET {
        text(api, 12, 50, b"NO POSITION SET", YELLOW, BLACK, 1);
        text(api, 12, 66, b"ENTER IT IN SETTINGS", GRAY, BLACK, 0);
    } else {
        let g = grid_locator(lat, lon);
        text(api, 30, 40, &g, GREEN, BLACK, 2);
    }
    footer(api, s, b"A/B:NEXT TOOL");
}

fn draw(api: &Api, s: &State) {
    (api.fill_rect)(0, 0, 160, 128, BLACK);
    match s.tool {
        Tool::Id => draw_id(api, s),
        Tool::Watch => draw_watch(api, s),
        Tool::Wave => draw_wave(api, s),
        Tool::Grid => draw_grid(api, s),
    }
}

// ------------------------------------------------------------------- events

fn busy(s: &State) -> bool {
    s.id.armed || s.sw.running
}

fn key(s: &mut State, id: u8, kind: u8) -> AppResult {
    s.note = b"";
    if id == KEY_EXIT {
        if kind == KIND_LONG || (kind == KIND_SINGLE && !busy(s)) {
            return AppResult::Exit;
        }
        if kind == KIND_SINGLE {
            s.note = b"HOLD EXIT: TIMER RUNS";
        }
        return AppResult::Continue;
    }
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    if id == KEY_AB {
        s.tool = next(s.tool);
        return AppResult::Continue;
    }
    match s.tool {
        Tool::Id => match id {
            KEY_MENU => s.id.identified(),
            KEY_STAR => {
                s.id.armed = !s.id.armed;
                s.id.identified();
            }
            KEY_UP => s.id.adjust(true),
            KEY_DOWN => s.id.adjust(false),
            _ => {}
        },
        Tool::Watch => match id {
            KEY_MENU => s.sw.running = !s.sw.running,
            KEY_STAR => s.sw.lap(),
            0 => s.sw.reset(),
            _ => {}
        },
        Tool::Wave => match id {
            0..=9 => {
                s.khz = push_digit(s.khz, id, s.fresh);
                s.fresh = false;
            }
            KEY_STAR => {
                s.khz = 0;
                s.fresh = false;
            }
            _ => {}
        },
        Tool::Grid => {}
    }
    AppResult::Continue
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    let s = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match ev {
        AppEvent::Enter | AppEvent::Leave => {
            (api.set_backlight_hold)(false);
            AppResult::Continue
        }
        AppEvent::Tick { dt_100us } => {
            s.clock = s.clock.wrapping_add(dt_100us);
            s.id.advance(dt_100us);
            s.sw.advance(dt_100us);
            (api.set_backlight_hold)(s.id.due());
            AppResult::Continue
        }
        AppEvent::Draw => {
            draw(api, s);
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => key(s, id, kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavelength_known_values() {
        assert_eq!(wavelength_mm(146_520), 2046); // 2 m band
        assert_eq!(wavelength_mm(446_000), 672);
        assert_eq!(wavelength_mm(0), 0);
    }

    #[test]
    fn grid_known_locators() {
        // Reference values computed independently (floating point) with the Maidenhead definition
        assert_eq!(&grid_locator(5_150_000, 0), b"JO01am");
        assert_eq!(&grid_locator(4_071_000, -7_400_000), b"FN30ar"); // New York
        assert_eq!(&grid_locator(-3_386_000, 15_121_000), b"QF56od"); // Sydney
    }

    #[test]
    fn id_timer_counts_down_and_goes_overdue() {
        let mut t = IdTimer::new();
        t.armed = true;
        t.advance(599 * TICKS_PER_S);
        assert!(!t.due());
        assert_eq!(t.display_s(), 1);
        t.advance(2 * TICKS_PER_S);
        assert!(t.due());
        assert_eq!(t.display_s(), 1); // one second overdue
        t.identified();
        assert!(!t.due());
        assert_eq!(t.display_s(), 600);
    }

    #[test]
    fn id_timer_ignores_time_when_disarmed_and_clamps_interval() {
        let mut t = IdTimer::new();
        t.advance(1_000_000);
        assert_eq!(t.elapsed, 0);
        for _ in 0..40 {
            t.adjust(true);
        }
        assert_eq!(t.interval_s, ID_MAX_S);
        for _ in 0..40 {
            t.adjust(false);
        }
        assert_eq!(t.interval_s, ID_MIN_S);
        t.armed = true;
        t.adjust(true);
        assert_eq!(t.interval_s, ID_MIN_S); // locked while armed
    }

    #[test]
    fn stopwatch_laps_roll_over() {
        let mut w = Stopwatch::new();
        w.lap();
        assert_eq!(w.lap_count, 0); // not running
        w.running = true;
        for i in 1..=6u32 {
            w.advance(i * 10_000);
            w.lap();
        }
        assert_eq!(w.lap_count, LAPS);
        assert_eq!(w.laps[LAPS - 1], w.elapsed);
        assert!(w.laps[0] < w.laps[1]);
    }

    #[test]
    fn stopwatch_saturates_at_99_59() {
        let mut w = Stopwatch::new();
        w.running = true;
        w.advance(u32::MAX);
        assert_eq!(w.elapsed, SW_MAX);
    }

    #[test]
    fn digit_entry_limits_length() {
        let mut k = 0;
        for d in [1, 4, 6, 5, 2, 0, 9, 9] {
            k = push_digit(k, d, k == 0);
        }
        assert_eq!(k, 146_520);
        assert_eq!(push_digit(146_520, 7, true), 7); // fresh start replaces
    }

    #[test]
    fn mmss_formats() {
        let mut b = [0u8; 5];
        fmt_mmss(605, &mut b);
        assert_eq!(&b, b"10:05");
        fmt_mmss(100 * 60, &mut b);
        assert_eq!(&b, b"99:00");
    }
}
