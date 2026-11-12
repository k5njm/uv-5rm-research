//! Band Sweep: repeated passes over a whole band, far wider than Spectrum's
//! 128 bins. Keeps only the strongest reading per screen column plus a short
//! list of exact frequencies, so a 9600-step pass fits the 8 KB app arena.

use bf5rh_abi::{Api, AppEvent, AppResult};

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KIND_REPEAT: u8 = 4;
const KEY_ASTERISK: u8 = 10;
const KEY_POUND: u8 = 11;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_VM: u8 = 16; // the VFO/MR key
const KEY_BAND: u8 = 18; // the BAND key (the green band/channels key)
const KEY_SIDE1: u8 = 19;

const F5: u8 = 0;
const F6: u8 = 1; // 6x10

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const GRID: u16 = 0x2124;
const TRACE: u16 = 0x07E0; // green, as SPECTRUM: red is for transmitting
const HOLD: u16 = 0x0320;
const SELECT_BG: u16 = 0x231A; // blue: you are here
const AMBER: u16 = 0xFD40; // you are changing this
// The house look of the list screens (src/ui/list.rs): light title bar, dark hint strip.
const TITLE_BG: u16 = 0xDEDB;
const TITLE_FG: u16 = 0x0000;
const HINT_BG: u16 = 0x2124;
const HINT_FG: u16 = 0xA514;

const COLS: usize = 160;
const NO_MARK: u8 = 255;
const SCREEN_H: u16 = 128;
const TITLE_H: u16 = 14;
const GRAPH_TOP: i16 = 15;
const GRAPH_BOTTOM: i16 = 55;
const CURSOR_Y: i16 = 56;
const LIST_Y: i16 = 58;
const ROW_H: i16 = 9;
const LIST_ROWS: usize = 5;
const STATUS_Y: i16 = 104;
const HINT_TOP: i16 = 112;

/// `(name, low Hz, high Hz, step index)`, in frequency order; the last is the typed custom range. Every preset lies inside
/// what the receiver hears (`app/group_table.rs` `RX_BANDS`: 108-174, 200-260, 350-520 MHz), and the named ones use the
/// core's own `SCAN_BANDS` edges and names, so this app and the main screen agree. The step is the one that fits the
/// band's channel raster: 25 kHz for the wide blocks, air (25 kHz AM channels; 8.33 kHz channels fall between two grid
/// points and are found at the nearer one) and the marine and weather channels; 12.5 kHz for 2 m, MURS, 1.25 m, 70 cm and
/// GMRS, where the narrow channels are. The 1 key still switches the step.
const BANDS: [(&[u8], u32, u32, u8); 16] = [
    (b"AIR", 108_000_000, 136_000_000, 0),
    (b"136-144", 136_000_000, 144_000_000, 0),
    (b"2M", 144_000_000, 148_000_000, 1),
    (b"148-174", 148_000_000, 174_000_000, 0),
    (b"MURS", 151_820_000, 154_600_000, 1),
    (b"MARINE", 156_000_000, 162_025_000, 0),
    (b"WX", 162_400_000, 162_550_000, 0),
    (b"200-222", 200_000_000, 222_000_000, 0),
    (b"1.25M", 222_000_000, 225_000_000, 1),
    (b"225-260", 225_000_000, 260_000_000, 0),
    (b"350-420", 350_000_000, 420_000_000, 0),
    (b"70CM", 420_000_000, 450_000_000, 1),
    (b"450-462", 450_000_000, 462_550_000, 0),
    (b"GMRS", 462_550_000, 467_725_000, 1),
    (b"467-520", 467_725_000, 520_000_000, 0),
    (b"CUSTOM", 144_000_000, 148_000_000, 0),
];
const CUSTOM: usize = BANDS.len() - 1;

const STEPS_HZ: [u32; 2] = [25_000, 12_500];

const RX_MIN_HZ: u32 = 108_000_000;
const RX_MAX_HZ: u32 = 520_000_000;
const AM_LOW_HZ: u32 = 108_000_000;
const AM_HIGH_HZ: u32 = 136_000_000;

const RSSI_DBM_BASE: i32 = 170;
/// dB of the graph kept below the noise floor, so noise reads as low grass.
const GRAPH_BELOW_FLOOR: u8 = 4;
const GRAPH_RAW_SPAN: u32 = 70;

const DEFAULT_DWELL_100US: u16 = 30;
const DWELL_MIN_100US: u16 = 10;
const DWELL_MAX_100US: u16 = 120;
const CONFIRM_100US: u16 = 10;
const TICK_BUDGET_100US: u32 = 500;

const DEFAULT_MARGIN: u8 = 16;
const MARGIN_MIN: u8 = 4;
const MARGIN_MAX: u8 = 40;
const INITIAL_FLOOR: u8 = 70; // -100 dBm
const HIST: usize = 128;
const FLOOR_EVERY: u16 = 256;

const MAX_HITS: usize = 24;
const MAX_IGNORED: usize = 8;

const INPUT_DIGITS: usize = 6;
const AGC_SWEEP: i8 = 0;
const AGC_AUTO: i8 = -1;

#[derive(Clone, Copy, PartialEq)]
struct Hit {
    hz: u32,
    peak: u8,
    count: u8,
    last_pass: u16,
}

const NO_HIT: Hit = Hit {
    hz: 0,
    peak: 0,
    count: 0,
    last_pass: 0,
};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Sweep,
    Listen,
    /// Typing the custom range; the value is 0 for start, 1 for stop.
    Input(u8),
}

struct State {
    view: View,
    band: usize,
    custom_lo: u32,
    custom_hi: u32,
    step_index: usize,
    dwell_100us: u16,
    margin: u8,

    lo_hz: u32,
    total: u32,
    pos: u32,
    pass: u16,
    pass_start_100us: u32,
    /// Tenths of a second the last full pass took; 0 until one has finished.
    pass_tenths: u32,

    hist: [u16; HIST],
    samples: u16,
    floor: u8,
    graph_base: u8,

    cur: [u8; COLS],
    hold: [u8; COLS],
    dirty_lo: usize,
    dirty_hi: usize,
    cursor_col: usize,
    /// Graph column of the selected hit's pointer as last drawn (`NO_MARK` when none).
    mark: u8,

    hits: [Hit; MAX_HITS],
    n_hits: usize,
    selected: usize,
    scroll: usize,
    ignored: [u32; MAX_IGNORED],
    n_ignored: usize,

    run_hz: u32,
    run_peak: u8,
    run_pos: u32,

    listen_hz: u32,
    input: [u8; INPUT_DIGITS],
    input_len: usize,

    list_dirty: bool,
    status_dirty: bool,
    full: bool,
}

static mut STATE: State = State {
    view: View::Sweep,
    band: 0,
    custom_lo: BANDS[CUSTOM].1,
    custom_hi: BANDS[CUSTOM].2,
    step_index: 0,
    dwell_100us: DEFAULT_DWELL_100US,
    margin: DEFAULT_MARGIN,
    lo_hz: 0,
    total: 0,
    pos: 0,
    pass: 0,
    pass_start_100us: 0,
    pass_tenths: 0,
    hist: [0; HIST],
    samples: 0,
    floor: INITIAL_FLOOR,
    graph_base: INITIAL_FLOOR - GRAPH_BELOW_FLOOR,
    cur: [0; COLS],
    hold: [0; COLS],
    dirty_lo: COLS,
    dirty_hi: 0,
    cursor_col: 0,
    mark: NO_MARK,
    hits: [NO_HIT; MAX_HITS],
    n_hits: 0,
    selected: 0,
    scroll: 0,
    ignored: [0; MAX_IGNORED],
    n_ignored: 0,
    run_hz: 0,
    run_peak: 0,
    run_pos: 0,
    listen_hz: 0,
    input: [0; INPUT_DIGITS],
    input_len: 0,
    list_dirty: true,
    status_dirty: true,
    full: true,
};

fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { .. } => tick(api),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => leave(api),
    }
}

impl State {
    fn step_hz(&self) -> u32 {
        STEPS_HZ[self.step_index]
    }

    fn band_range(&self) -> (u32, u32) {
        if self.band == CUSTOM {
            (self.custom_lo, self.custom_hi)
        } else {
            (BANDS[self.band].1, BANDS[self.band].2)
        }
    }

    fn trigger(&self) -> u16 {
        self.floor as u16 + self.margin as u16
    }

    /// Everything measured belongs to one band and step: start over.
    fn restart(&mut self, now: u32) {
        let (lo, hi) = self.band_range();
        self.lo_hz = lo;
        self.total = (hi - lo) / self.step_hz() + 1;
        self.pos = 0;
        self.pass = 0;
        self.pass_tenths = 0;
        self.pass_start_100us = now;
        self.hist = [0; HIST];
        self.samples = 0;
        self.floor = INITIAL_FLOOR;
        self.graph_base = INITIAL_FLOOR - GRAPH_BELOW_FLOOR;
        self.cur = [0; COLS];
        self.hold = [0; COLS];
        self.dirty_lo = COLS;
        self.dirty_hi = 0;
        self.cursor_col = 0;
        self.hits = [NO_HIT; MAX_HITS];
        self.n_hits = 0;
        self.selected = 0;
        self.scroll = 0;
        self.run_peak = 0;
        self.full = true;
    }

    fn update_floor(&mut self) {
        let target = (self.samples as u32 / 2).max(1);
        let mut acc: u32 = 0;
        for (bucket, &count) in self.hist.iter().enumerate() {
            acc += count as u32;
            if acc >= target {
                self.floor = (bucket * 2).min(255) as u8;
                return;
            }
        }
    }

    /// Rescaling moves every bar, so it happens rarely: once the first floor
    /// reading arrives, then at the end of each pass.
    fn rescale_graph(&mut self) {
        let base = self.floor.saturating_sub(GRAPH_BELOW_FLOOR);
        if base != self.graph_base {
            self.graph_base = base;
            self.dirty_lo = 0;
            self.dirty_hi = COLS - 1;
        }
    }

    fn is_ignored(&self, hz: u32) -> bool {
        let near = self.step_hz();
        self.ignored[..self.n_ignored]
            .iter()
            .any(|&f| f.abs_diff(hz) <= near)
    }

    /// Strongest first, keeping the cursor on the frequency it was on.
    fn sort_hits(&mut self) {
        let selected_hz = if self.n_hits > 0 {
            self.hits[self.selected].hz
        } else {
            0
        };
        for i in 1..self.n_hits {
            let mut j = i;
            while j > 0 && self.hits[j].peak > self.hits[j - 1].peak {
                self.hits.swap(j, j - 1);
                j -= 1;
            }
        }
        if let Some(i) = self.hits[..self.n_hits]
            .iter()
            .position(|h| h.hz == selected_hz)
        {
            self.selected = i;
        }
    }

    fn commit_hit(&mut self, hz: u32, peak: u8) {
        if self.is_ignored(hz) {
            return;
        }
        let near = self.step_hz();
        let pass = self.pass;
        let found = self.hits[..self.n_hits]
            .iter()
            .position(|h| h.hz.abs_diff(hz) <= near);
        match found {
            Some(i) => {
                let h = &mut self.hits[i];
                if peak > h.peak {
                    h.peak = peak;
                    h.hz = hz;
                }
                if h.last_pass != pass {
                    h.last_pass = pass;
                    h.count = h.count.saturating_add(1);
                }
            }
            None => {
                let new = Hit {
                    hz,
                    peak,
                    count: 1,
                    last_pass: pass,
                };
                if self.n_hits < MAX_HITS {
                    self.hits[self.n_hits] = new;
                    self.n_hits += 1;
                } else if peak > self.hits[MAX_HITS - 1].peak {
                    // Sorted, so the last entry is the weakest.
                    self.hits[MAX_HITS - 1] = new;
                } else {
                    return;
                }
            }
        }
        self.sort_hits();
        self.list_dirty = true;
    }

    fn end_run(&mut self) {
        if self.run_peak > 0 {
            let (hz, peak) = (self.run_hz, self.run_peak);
            self.run_peak = 0;
            self.commit_hit(hz, peak);
        }
    }

    fn remove_selected(&mut self) {
        if self.n_hits == 0 {
            return;
        }
        let hz = self.hits[self.selected].hz;
        if self.n_ignored < MAX_IGNORED {
            self.ignored[self.n_ignored] = hz;
            self.n_ignored += 1;
        }
        for i in self.selected..self.n_hits - 1 {
            self.hits[i] = self.hits[i + 1];
        }
        self.n_hits -= 1;
        if self.selected >= self.n_hits {
            self.selected = self.n_hits.saturating_sub(1);
        }
        self.list_dirty = true;
    }
}

fn start_sweep_rx(api: &Api) {
    (api.set_modulation)(0);
    (api.set_bandwidth)(true);
    (api.enter_rx)();
    (api.set_holds_rx)(false);
    (api.set_af_out)(false);
    (api.set_agc_fix)(AGC_SWEEP);
}

fn enter(api: &Api) -> AppResult {
    let st = state();
    st.view = View::Sweep;
    st.input_len = 0;
    st.n_ignored = 0;
    // Open on the narrowest band the radio is already tuned into, when there is one.
    let hz = (api.get_master_freq)();
    let mut best = 0;
    let mut span = u32::MAX;
    for (i, b) in BANDS[..CUSTOM].iter().enumerate() {
        if (b.1..=b.2).contains(&hz) && b.2 - b.1 < span {
            best = i;
            span = b.2 - b.1;
        }
    }
    set_band(api, st, best);
    start_sweep_rx(api);
    (api.set_backlight_hold)(true);
    AppResult::Continue
}

/// Switch to preset `band` with its own step, and start the sweep over.
fn set_band(api: &Api, st: &mut State, band: usize) {
    st.band = band;
    st.step_index = BANDS[band].3 as usize;
    st.restart((api.uptime_100us)());
}

fn leave(api: &Api) -> AppResult {
    (api.set_af_out)(false);
    (api.set_agc_fix)(AGC_AUTO);
    (api.set_backlight_hold)(false);
    AppResult::Continue
}

fn dwell(api: &Api, units: u16) {
    let t0 = (api.uptime_100us)();
    while (api.uptime_100us)().wrapping_sub(t0) < units as u32 {}
}

fn step(api: &Api, st: &mut State) {
    let pos = st.pos;
    let hz = st.lo_hz + pos * st.step_hz();
    (api.retune_rx)(hz, true);
    dwell(api, st.dwell_100us);
    let mut rssi = (api.read_rssi)();
    if rssi >= st.trigger() {
        // One noisy read shouldn't make a hit.
        dwell(api, CONFIRM_100US);
        rssi = rssi.min((api.read_rssi)());
    }
    let raw = rssi.min(255) as u8;

    let col = (pos as u64 * COLS as u64 / st.total as u64) as usize;
    if raw > st.cur[col] {
        st.cur[col] = raw;
    }
    if raw > st.hold[col] {
        st.hold[col] = raw;
    }
    st.dirty_lo = st.dirty_lo.min(col);
    st.dirty_hi = st.dirty_hi.max(col);
    st.cursor_col = col;

    let bucket = (raw as usize / 2).min(HIST - 1);
    st.hist[bucket] = st.hist[bucket].saturating_add(1);
    st.samples = st.samples.saturating_add(1);
    if st.samples % FLOOR_EVERY == 0 {
        st.update_floor();
        if st.pass == 0 && st.samples == FLOOR_EVERY {
            st.rescale_graph();
        }
        st.status_dirty = true;
    }

    if rssi >= st.trigger() {
        // Adjacent steps are one signal: keep its strongest frequency.
        if st.run_peak > 0 && st.run_pos + 1 != pos {
            st.end_run();
        }
        if raw > st.run_peak {
            st.run_peak = raw;
            st.run_hz = hz;
        }
        st.run_pos = pos;
    } else {
        st.end_run();
    }

    st.pos += 1;
    if st.pos >= st.total {
        finish_pass(api, st);
    }
}

fn finish_pass(api: &Api, st: &mut State) {
    st.end_run();
    st.update_floor();
    st.rescale_graph();
    let now = (api.uptime_100us)();
    st.pass_tenths = now.wrapping_sub(st.pass_start_100us) / 1000;
    st.pass_start_100us = now;
    st.pass = st.pass.wrapping_add(1);
    st.pos = 0;
    st.hist = [0; HIST];
    st.samples = 0;
    st.cur = [0; COLS];
    st.dirty_lo = 0;
    st.dirty_hi = COLS - 1;
    st.status_dirty = true;
}

fn tick(api: &Api) -> AppResult {
    let st = state();
    match st.view {
        View::Sweep => {
            let t0 = (api.uptime_100us)();
            loop {
                step(api, st);
                if st.pos == 0
                    || (api.uptime_100us)().wrapping_sub(t0)
                        >= TICK_BUDGET_100US
                {
                    break;
                }
            }
        }
        View::Listen => st.status_dirty = true,
        View::Input(_) => {}
    }
    AppResult::Continue
}

fn start_listen(api: &Api, st: &mut State) {
    if st.n_hits == 0 {
        return;
    }
    st.listen_hz = st.hits[st.selected].hz;
    st.view = View::Listen;
    st.status_dirty = true;
    let am = (AM_LOW_HZ..=AM_HIGH_HZ).contains(&st.listen_hz);
    (api.set_agc_fix)(AGC_AUTO);
    (api.set_modulation)(if am { 1 } else { 0 });
    (api.set_rx_freq)(st.listen_hz);
    (api.set_bandwidth)(true);
    (api.set_subaudio_rx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
}

fn stop_listen(api: &Api, st: &mut State) {
    st.view = View::Sweep;
    st.status_dirty = true;
    st.list_dirty = true;
    st.pos = 0;
    st.run_peak = 0;
    st.pass_start_100us = (api.uptime_100us)();
    start_sweep_rx(api);
}

/// `app/group_table.rs` `RX_BANDS`: what the receiver hears. A custom range must start and stop inside one block.
fn receivable(hz: u32) -> bool {
    (RX_MIN_HZ..=174_000_000).contains(&hz) || (200_000_000..=260_000_000).contains(&hz) || (350_000_000..=RX_MAX_HZ).contains(&hz)
}

fn commit_input(api: &Api, st: &mut State, stage: u8) {
    let mut khz: u32 = 0;
    for i in 0..INPUT_DIGITS {
        let d = if i < st.input_len { st.input[i] } else { 0 };
        khz = khz * 10 + d as u32;
    }
    let typed = st.input_len > 0;
    st.input_len = 0;
    if typed {
        let hz = khz * 1000;
        if stage == 0 {
            st.custom_lo = hz;
        } else {
            st.custom_hi = hz;
        }
    }
    if stage == 0 {
        st.view = View::Input(1);
    } else {
        // Anything out of the receiver's range goes back to the start field.
        let (lo, hi) = (st.custom_lo, st.custom_hi);
        if lo < hi && receivable(lo) && receivable(hi) {
            st.band = CUSTOM;
            st.view = View::Sweep;
            st.restart((api.uptime_100us)());
            return;
        }
        st.view = View::Input(0);
    }
    st.status_dirty = true;
}

fn input_key(api: &Api, st: &mut State, id: u8, stage: u8) {
    match id {
        0..=9 => {
            if st.input_len < INPUT_DIGITS {
                st.input[st.input_len] = id;
                st.input_len += 1;
            }
            if st.input_len == INPUT_DIGITS {
                commit_input(api, st, stage);
            }
        }
        KEY_MENU => commit_input(api, st, stage),
        KEY_EXIT => {
            if st.input_len > 0 {
                st.input_len = 0;
            } else {
                st.view = View::Sweep;
                st.full = true;
            }
        }
        _ => {}
    }
    st.status_dirty = true;
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    let st = state();
    if let View::Input(stage) = st.view {
        if kind == KIND_SINGLE {
            input_key(api, st, id, stage);
        }
        return AppResult::Continue;
    }
    if st.view == View::Listen {
        if kind == KIND_SINGLE && id == KEY_VM {
            // Tune the main screen's VFO to what is being heard (no tone) and leave for the main screen: the core returns
            // there, not to the APPS list, because the app handed over a frequency (`api_save_master_vfo`).
            (api.save_master_vfo)(st.listen_hz, 0);
            return AppResult::Exit;
        }
        if kind == KIND_SINGLE && matches!(id, KEY_MENU | KEY_EXIT) {
            stop_listen(api, st); // MENU again or EXIT: stop listening, the sweep carries on
        }
        return AppResult::Continue;
    }
    if kind == KIND_REPEAT && matches!(id, KEY_UP | KEY_DOWN) {
        move_selection(st, id == KEY_DOWN);
        return AppResult::Continue;
    }
    if kind == KIND_LONG && id == KEY_BAND {
        // hold BAND: the band before this one (BAND steps forward)
        let prev = if st.band == 0 || st.band >= CUSTOM { CUSTOM - 1 } else { st.band - 1 };
        set_band(api, st, prev);
        return AppResult::Continue;
    }
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    match id {
        KEY_EXIT => return AppResult::Exit,
        KEY_UP | KEY_DOWN => move_selection(st, id == KEY_DOWN),
        KEY_MENU => start_listen(api, st),
        KEY_VM => {
            if st.n_hits > 0 {
                (api.save_master_vfo)(st.hits[st.selected].hz, 0);
                return AppResult::Exit;
            }
        }
        KEY_BAND => {
            let next = if st.band >= CUSTOM - 1 { 0 } else { st.band + 1 };
            set_band(api, st, next);
        }
        KEY_SIDE1 => st.remove_selected(),
        KEY_ASTERISK => {
            st.margin = (st.margin + 2).min(MARGIN_MAX);
            st.status_dirty = true;
        }
        KEY_POUND => {
            st.margin = st.margin.saturating_sub(2).max(MARGIN_MIN);
            st.status_dirty = true;
        }
        0 => {
            st.dwell_100us = (st.dwell_100us + 10).min(DWELL_MAX_100US);
            st.status_dirty = true;
        }
        6 => {
            st.dwell_100us = st.dwell_100us.saturating_sub(10).max(DWELL_MIN_100US);
            st.status_dirty = true;
        }
        1 => {
            st.step_index = (st.step_index + 1) % STEPS_HZ.len();
            st.restart((api.uptime_100us)());
        }
        5 => {
            st.input_len = 0;
            st.view = View::Input(0);
            st.status_dirty = true;
        }
        _ => {}
    }
    AppResult::Continue
}

fn move_selection(st: &mut State, down: bool) {
    if st.n_hits == 0 {
        return;
    }
    st.selected = if down {
        (st.selected + 1) % st.n_hits
    } else {
        (st.selected + st.n_hits - 1) % st.n_hits
    };
    st.list_dirty = true;
}

// ui

struct Buf {
    b: [u8; 34],
    n: usize,
}

impl Buf {
    fn new() -> Buf {
        Buf { b: [0; 34], n: 0 }
    }

    fn ch(&mut self, c: u8) {
        if self.n < self.b.len() {
            self.b[self.n] = c;
            self.n += 1;
        }
    }

    fn s(&mut self, text: &[u8]) {
        for &c in text {
            self.ch(c);
        }
    }

    fn u(&mut self, api: &Api, v: u32) {
        let mut tmp = [0u8; 12];
        let n = (api.fmt_u32)(v, tmp.as_mut_ptr()) as usize;
        self.s(&tmp[..n.min(tmp.len())]);
    }

    fn i(&mut self, api: &Api, v: i32) {
        if v < 0 {
            self.ch(b'-');
            self.u(api, (-v) as u32);
        } else {
            self.u(api, v as u32);
        }
    }

    fn freq(&mut self, api: &Api, hz: u32) {
        let mut tmp = [0u8; 16];
        let n = (api.fmt_freq)(hz, tmp.as_mut_ptr()) as usize;
        self.s(&tmp[..n.min(tmp.len())]);
    }

    fn as_slice(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16, font: u8) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, font);
}

fn line(api: &Api, y: i16, s: &[u8], fg: u16, bg: u16) {
    (api.fill_rect)(0, y, COLS as u16, 8, bg);
    text(api, 1, y, s, fg, bg, F5);
}

fn raw_to_y(raw: u8, base: u8) -> i16 {
    let v = (raw as u32).saturating_sub(base as u32).min(GRAPH_RAW_SPAN);
    let h = (GRAPH_BOTTOM - GRAPH_TOP) as u32;
    GRAPH_BOTTOM - (v * h / GRAPH_RAW_SPAN) as i16
}

fn draw_column(api: &Api, st: &State, x: usize) {
    let top = raw_to_y(st.cur[x], st.graph_base);
    let peak = raw_to_y(st.hold[x], st.graph_base);
    let xi = x as i16;
    let height = (GRAPH_BOTTOM - GRAPH_TOP + 1) as u16;
    (api.fill_rect)(xi, GRAPH_TOP, 1, height, BLACK);
    if peak < top {
        (api.fill_rect)(xi, peak, 1, (top - peak) as u16, HOLD);
    }
    if st.cur[x] > 0 {
        (api.fill_rect)(xi, top, 1, (GRAPH_BOTTOM - top + 1) as u16, TRACE);
    }
    // The selected hit's pointer, as SPECTRUM marks its peak: an amber line down to the trace and a small triangle
    // at the top edge. It is drawn here, in the pass that repaints the column, so no sweep pass leaves it stale.
    if st.mark != NO_MARK {
        let m = st.mark as i16;
        if xi == m && top > GRAPH_TOP {
            (api.fill_rect)(xi, GRAPH_TOP, 1, (top - GRAPH_TOP) as u16, AMBER);
        }
        // the triangle stays whole at the first and last columns (its tip may then sit 1 or 2 columns off the line)
        let dx = (xi - m.clamp(2, COLS as i16 - 3)).abs();
        if dx <= 2 {
            (api.fill_rect)(xi, GRAPH_TOP, 1, (3 - dx) as u16, AMBER);
        }
    }
}

/// The graph column of the selected hit, by the formula `step` files each reading under (position * COLS / total).
fn mark_col(st: &State) -> u8 {
    if st.n_hits == 0 || st.total == 0 {
        return NO_MARK;
    }
    let pos = st.hits[st.selected].hz.saturating_sub(st.lo_hz) / st.step_hz();
    (pos as u64 * COLS as u64 / st.total as u64).min(COLS as u64 - 1) as u8
}

/// Light title bar as on the list screens: band start, band name, band stop (4 decimals, so all three fit).
fn draw_header(api: &Api, st: &State) {
    let (lo, hi) = st.band_range();
    (api.fill_rect)(0, 0, COLS as u16, TITLE_H, TITLE_BG);
    let mut left = Buf::new();
    left.freq(api, lo);
    left.n = left.n.saturating_sub(1);
    text(api, 2, 2, left.as_slice(), TITLE_FG, TITLE_BG, F6);
    let name = if st.band == CUSTOM { b"CUSTOM" as &[u8] } else { BANDS[st.band].0 };
    text(api, (COLS as i16 - name.len() as i16 * 6) / 2, 2, name, TITLE_FG, TITLE_BG, F6);
    let mut right = Buf::new();
    right.freq(api, hi);
    right.n = right.n.saturating_sub(1);
    let w = right.n as i16 * 6;
    text(api, COLS as i16 - 2 - w, 2, right.as_slice(), TITLE_FG, TITLE_BG, F6);
}

/// What the keys do now, in the strip every list screen has at the bottom.
fn draw_hint(api: &Api, s: &[u8]) {
    (api.fill_rect)(0, HINT_TOP, COLS as u16, 16, HINT_BG);
    text(api, 4, HINT_TOP + 3, s, HINT_FG, HINT_BG, F6);
}

fn draw_list(api: &Api, st: &mut State) {
    if st.selected < st.scroll {
        st.scroll = st.selected;
    } else if st.selected >= st.scroll + LIST_ROWS {
        st.scroll = st.selected + 1 - LIST_ROWS;
    }
    for slot in 0..LIST_ROWS {
        let y = LIST_Y + slot as i16 * ROW_H;
        let i = st.scroll + slot;
        if i >= st.n_hits {
            (api.fill_rect)(0, y, COLS as u16, ROW_H as u16, BLACK);
            if slot == 0 {
                text(api, 1, y, b"NO SIGNALS YET", GRAY, BLACK, F5);
            }
            continue;
        }
        let h = st.hits[i];
        let bg = if i == st.selected { SELECT_BG } else { BLACK };
        let mut b = Buf::new();
        b.freq(api, h.hz);
        b.s(b"  ");
        b.i(api, h.peak as i32 - RSSI_DBM_BASE);
        b.s(b"DBM X");
        b.u(api, h.count as u32);
        (api.fill_rect)(0, y, COLS as u16, ROW_H as u16, bg);
        text(api, 1, y, b.as_slice(), WHITE, bg, F5);
    }
}

fn draw_status(api: &Api, st: &State) {
    let mut a = Buf::new();
    match st.view {
        View::Input(stage) => {
            a.s(if stage == 0 { b"START " } else { b"STOP " });
            for i in 0..INPUT_DIGITS {
                if i == 3 {
                    a.ch(b'.');
                }
                a.ch(if i < st.input_len {
                    b'0' + st.input[i]
                } else {
                    b'-'
                });
            }
            a.s(b" MHZ");
            line(api, STATUS_Y, a.as_slice(), AMBER, BLACK);
            draw_hint(api, if stage == 0 { b"MENU next  EXIT back" } else { b"MENU sweep  EXIT back" });
            return;
        }
        View::Listen => {
            a.s(b"RX ");
            a.freq(api, st.listen_hz);
            a.ch(b' ');
            a.i(api, (api.read_rssi)() as i32 - RSSI_DBM_BASE);
            a.s(b"DBM");
            line(api, STATUS_Y, a.as_slice(), TRACE, BLACK);
            draw_hint(api, b"MENU sweep  VFO/MR tune");
            return;
        }
        View::Sweep => {}
    }
    a.s(if st.step_index == 0 { b"ST25K" } else { b"ST12.5K" });
    a.s(b" T+");
    a.u(api, st.margin as u32);
    a.s(b" D");
    a.u(api, st.dwell_100us as u32 / 10);
    a.s(b" P");
    a.u(api, st.pass as u32);
    if st.pass_tenths > 0 {
        a.ch(b' ');
        a.u(api, st.pass_tenths / 10);
        a.ch(b'.');
        a.u(api, st.pass_tenths % 10);
        a.ch(b'S');
    }
    line(api, STATUS_Y, a.as_slice(), GRAY, BLACK);
    draw_hint(
        api,
        if st.n_hits > 0 { b"MENU hear  VFO/MR tune" } else { b"BAND next  1 step  5 range" },
    );
}

fn draw(api: &Api) -> AppResult {
    let st = state();
    if st.full {
        (api.fill_rect)(0, 0, COLS as u16, SCREEN_H, BLACK);
        draw_header(api, st);
        st.dirty_lo = 0;
        st.dirty_hi = COLS - 1;
        st.list_dirty = true;
        st.status_dirty = true;
        st.full = false;
    }
    let want = mark_col(st);
    if want != st.mark {
        // repaint the columns the pointer leaves and the ones it arrives on (the triangle is 5 wide)
        for c in [st.mark, want] {
            if c != NO_MARK {
                st.dirty_lo = st.dirty_lo.min((c as usize).saturating_sub(2));
                st.dirty_hi = st.dirty_hi.max((c as usize + 2).min(COLS - 1));
            }
        }
        st.mark = want;
    }
    if st.dirty_lo <= st.dirty_hi {
        for x in st.dirty_lo..=st.dirty_hi {
            draw_column(api, st, x);
        }
        st.dirty_lo = COLS;
        st.dirty_hi = 0;
        (api.fill_rect)(0, CURSOR_Y, COLS as u16, 1, GRID);
        (api.fill_rect)(st.cursor_col.min(COLS - 2) as i16, CURSOR_Y, 2, 1, WHITE);
    }
    if st.list_dirty {
        draw_list(api, st);
        st.list_dirty = false;
        st.status_dirty = true; // the hint depends on whether there are hits
    }
    if st.status_dirty {
        draw_status(api, st);
        st.status_dirty = false;
    }
    AppResult::Continue
}

// Read-only views for the PC simulator; not part of the radio build.
#[cfg(not(target_os = "none"))]
pub fn debug_dump() -> std::string::String {
    use std::fmt::Write;
    let st = state();
    let mut out = std::format!("n={} floor={} ", st.n_hits, st.floor);
    for h in &st.hits[..st.n_hits] {
        let _ = write!(out, "hz={} peak={} x{} ", h.hz, h.peak, h.count);
    }
    let _ = write!(out, "pass_s={}", st.pass_tenths);
    out
}

#[cfg(not(target_os = "none"))]
pub fn debug_pass() -> std::string::String {
    let st = state();
    std::format!("{}.{} s, {} passes", st.pass_tenths / 10, st.pass_tenths % 10, st.pass)
}

#[cfg(not(target_os = "none"))]
pub fn debug_count() -> usize {
    state().n_hits
}

#[cfg(not(target_os = "none"))]
pub fn debug_selected_hz() -> u32 {
    let st = state();
    st.hits[st.selected].hz
}

#[cfg(not(target_os = "none"))]
pub fn debug_band() -> usize {
    state().band
}

#[cfg(not(target_os = "none"))]
pub fn debug_total() -> u32 {
    state().total
}

#[cfg(not(target_os = "none"))]
pub fn debug_lo() -> u32 {
    state().lo_hz
}
