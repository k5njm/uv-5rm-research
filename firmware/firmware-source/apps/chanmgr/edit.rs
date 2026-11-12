//! CHAN MGR segment 1: the channel editor. Opened from the list through the handoff (which slot, whether it is new,
//! where the list was); SAVE, DELETE and EXIT chain back to the list with the row to select.
use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};

use super::common::*;

impl Channel {
    fn wide_narrow(&self) -> bool {
        self.flags & 0x40 != 0
    }
    fn set_wide_narrow(&mut self, narrow: bool) {
        if narrow {
            self.flags |= 0x40;
        } else {
            self.flags &= !0x40;
        }
    }
    fn busy_lock(&self) -> bool {
        self.flags & 0x08 != 0
    }
    fn set_busy_lock(&mut self, on: bool) {
        if on {
            self.flags |= 0x08;
        } else {
            self.flags &= !0x08;
        }
    }
    fn scan_add(&self) -> bool {
        self.flags & 0x04 != 0
    }
    fn set_scan_add(&mut self, on: bool) {
        if on {
            self.flags |= 0x04;
        } else {
            self.flags &= !0x04;
        }
    }
    fn ani_target(&self) -> Option<u8> {
        let idx = self.dtmf_group & 0x1f;
        if (idx as usize) < CONTACT_COUNT {
            Some(idx)
        } else {
            None
        }
    }
    fn set_ani_target(&mut self, target: Option<u8>) {
        let idx = target.map_or(0x1f, |t| t.min(0x1f));
        self.dtmf_group = idx;
    }
}

/// Multi-tap accept delay, in 100us units (the `Tick` event's unit).
const MULTITAP_TIMEOUT_100US: u32 = 8_000;

const KEY_CHARS: [&[u8]; 10] = [
    b" ",
    b",.?1",
    b"ABCabc2",
    b"DEFdef3",
    b"GHIghi4",
    b"JKLjkl5",
    b"MNOmno6",
    b"PQRSpqrs7",
    b"TUVtuv8",
    b"WXYZwxyz9",
];

struct NameEdit<const N: usize> {
    buf: [u8; N],
    cursor: usize,
    pending: Option<(u8, usize)>,
    idle_100us: u32,
}

impl<const N: usize> NameEdit<N> {
    const fn blank() -> Self {
        NameEdit {
            buf: [0; N],
            cursor: 0,
            pending: None,
            idle_100us: 0,
        }
    }

    fn start(&mut self, initial: [u8; N]) {
        // Stock firmware and CHIRP pad names with 0xFF. Left as they are, no zero is found, the cursor starts
        // past the end and every key is refused (seen on a radio that came from stock firmware).
        self.buf = initial.map(|b| if b == 0xFF { 0 } else { b });
        self.cursor = self.buf.iter().position(|&b| b == 0).unwrap_or(N);
        self.pending = None;
        self.idle_100us = 0;
    }

    fn finalize_pending(&mut self) {
        if self.pending.take().is_some() {
            self.cursor = (self.cursor + 1).min(N);
        }
    }

    fn max_cursor(&self) -> usize {
        self.buf.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1)
    }

    fn move_cursor(&mut self, left: bool) {
        self.finalize_pending();
        self.cursor = if left {
            self.cursor.saturating_sub(1)
        } else {
            (self.cursor + 1).min(self.max_cursor())
        };
    }

    fn backspace(&mut self) {
        self.finalize_pending();
        if self.cursor == 0 {
            return;
        }
        for i in (self.cursor - 1)..(self.buf.len() - 1) {
            self.buf[i] = self.buf[i + 1];
        }
        *self.buf.last_mut().unwrap() = 0;
        self.cursor -= 1;
    }

    fn insert_at_cursor(&mut self, ch: u8) -> bool {
        if self.cursor >= N {
            return false;
        }
        let last = self.buf.len() - 1;
        for i in (self.cursor..last).rev() {
            self.buf[i + 1] = self.buf[i];
        }
        self.buf[self.cursor] = ch;
        true
    }

    fn press_digit(&mut self, digit: u8) {
        let table = KEY_CHARS[digit as usize];
        match self.pending {
            Some((d, idx)) if d == digit => {
                let next = (idx + 1) % table.len();
                self.buf[self.cursor] = table[next];
                self.pending = Some((digit, next));
            }
            _ => {
                self.finalize_pending();
                if self.insert_at_cursor(table[0]) {
                    self.pending = Some((digit, 0));
                }
            }
        }
        self.idle_100us = 0;
    }

    fn tick(&mut self, dt_100us: u32) {
        if self.pending.is_some() {
            self.idle_100us += dt_100us;
            if self.idle_100us >= MULTITAP_TIMEOUT_100US {
                self.finalize_pending();
            }
        }
    }
}

struct DigitInput<const N: usize> {
    digits: [u8; N],
    len: usize,
}

impl<const N: usize> DigitInput<N> {
    const fn new() -> Self {
        DigitInput {
            digits: [0; N],
            len: 0,
        }
    }
    fn clear(&mut self) {
        self.len = 0;
    }
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn is_full(&self) -> bool {
        self.len == N
    }
    fn push(&mut self, digit: u8) {
        if self.len < N {
            self.digits[self.len] = digit;
            self.len += 1;
        }
    }
    fn backspace(&mut self) {
        self.len = self.len.saturating_sub(1);
    }
    fn value(&self) -> u32 {
        let mut v: u32 = 0;
        for i in 0..N {
            let d = if i < self.len {
                self.digits[i] as u32
            } else {
                0
            };
            v = v * 10 + d;
        }
        v
    }

    fn write_display_into(&self, int_digits: usize, out: &mut [u8]) -> usize {
        let mut idx = 0;
        for i in 0..N {
            if i == int_digits {
                out[idx] = b'.';
                idx += 1;
            }
            out[idx] = if i < self.len {
                self.digits[i] + b'0'
            } else {
                b'-'
            };
            idx += 1;
        }
        idx
    }
}

fn digit_value(key: u8) -> Option<u8> {
    if key <= 9 {
        Some(key)
    } else {
        None
    }
}

fn clamp_step(cur: i32, up: bool, lo: i32, hi: i32) -> i32 {
    if up {
        (cur + 1).min(hi)
    } else {
        (cur - 1).max(lo)
    }
}

fn wrap_step(cur: i32, up: bool, lo: i32, hi: i32) -> i32 {
    if up {
        if cur >= hi {
            lo
        } else {
            cur + 1
        }
    } else if cur <= lo {
        hi
    } else {
        cur - 1
    }
}

fn power_norm(raw: u8) -> u8 {
    if raw <= 2 {
        raw
    } else {
        0
    }
}

fn power_label(raw: u8) -> &'static str {
    match raw {
        1 => "LOW",
        2 => "MID",
        _ => "HIGH",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Freq,
    Sftd,
    Offset,
    RxCts,
    TxCts,
    Power,
    Wn,
    ScanAdd,
    BusyLock,
    AniCall,
    Name,
    Save,
    Delete,
}

const BASE_FIELDS: [Field; 12] = [
    Field::Freq,
    Field::Sftd,
    Field::Offset,
    Field::RxCts,
    Field::TxCts,
    Field::Power,
    Field::Wn,
    Field::ScanAdd,
    Field::BusyLock,
    Field::AniCall,
    Field::Name,
    Field::Save,
];

fn field_slots(edit: &ChannelEdit) -> usize {
    BASE_FIELDS.len() + if edit.is_new { 0 } else { 1 }
}

fn field_at(index: usize) -> Field {
    if index < BASE_FIELDS.len() {
        BASE_FIELDS[index]
    } else {
        Field::Delete
    }
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Freq => "FREQUENCY",
            Field::Sftd => "RPT SHIFT",
            Field::Offset => "RPT OFFSET",
            Field::RxCts => "RX TONE",
            Field::TxCts => "TX TONE",
            Field::Power => "TX POWER",
            Field::Wn => "BANDWIDTH",
            Field::ScanAdd => "IN SCAN LIST",
            Field::BusyLock => "BUSY LOCKOUT",
            Field::AniCall => "ANI CALL",
            Field::Name => "NAME",
            Field::Save => "SAVE",
            Field::Delete => "DELETE",
        }
    }

    fn is_scalar(self) -> bool {
        matches!(
            self,
            Field::Sftd
                | Field::RxCts
                | Field::TxCts
                | Field::Power
                | Field::Wn
                | Field::ScanAdd
                | Field::BusyLock
                | Field::AniCall
        )
    }
}

struct ChannelEdit {
    num: u16,
    is_new: bool,
    working: Channel,
    sftd: u8,
    offset_hz: u32,
    field_index: usize,
    /// the group the list shows, and the row it had selected: where EXIT goes back to
    group: u8,
    ret_sel: u16,
    editing: bool,
    snapshot: i32,
    freq_input: DigitInput<8>,
    offset_input: DigitInput<7>,
    name_edit: NameEdit<12>,
}

fn derive_shift(ch: &Channel) -> (u8, u32) {
    let rx = ch.rx_freq_deci_hz();
    let tx = ch.tx_freq_deci_hz();
    if tx == rx {
        (0, 0)
    } else if tx > rx {
        (1, (tx - rx) * 10)
    } else {
        (2, (rx - tx) * 10)
    }
}

fn sync_tx_freq(edit: &mut ChannelEdit) {
    let rx_deci = edit.working.rx_freq_deci_hz();
    let tx_deci = match edit.sftd {
        1 => rx_deci + edit.offset_hz / 10,
        2 => rx_deci.saturating_sub(edit.offset_hz / 10),
        _ => rx_deci,
    };
    edit.working.set_tx_freq_deci_hz(tx_deci);
}

fn default_channel(freq_hz: u32) -> Channel {
    let mut ch = Channel::from_bytes(&[0u8; CHAN_SIZE]);
    let deci_hz = freq_hz / 10;
    ch.set_rx_freq_deci_hz(deci_hz);
    ch.set_tx_freq_deci_hz(deci_hz);
    ch.flags |= 0x04; // in the scan list: group scans skip channels without it
    ch.tx_power = 1; // LOW: raise it on purpose, not by default
    ch
}

impl ChannelEdit {
    fn open(num: u16, working: Channel, is_new: bool, group: u8, ret_sel: u16) -> Self {
        let (sftd, offset_hz) = derive_shift(&working);
        ChannelEdit {
            num,
            is_new,
            working,
            sftd,
            offset_hz,
            field_index: 0,
            group,
            ret_sel,
            editing: false,
            snapshot: 0,
            freq_input: DigitInput::new(),
            offset_input: DigitInput::new(),
            name_edit: NameEdit::blank(),
        }
    }

    fn is_editing(&self, index: usize) -> bool {
        self.editing && self.field_index == index
    }
}

fn current_value(api: &Api, edit: &ChannelEdit, field: Field) -> i32 {
    match field {
        Field::Sftd => edit.sftd as i32,
        Field::RxCts | Field::TxCts => {
            let raw = if field == Field::RxCts {
                edit.working.rx_dcs_cts_num
            } else {
                edit.working.tx_dcs_cts_num
            };
            (api.subaudio_index_of_code)(raw)
        }
        Field::Power => power_norm(edit.working.tx_power) as i32,
        Field::Wn => edit.working.wide_narrow() as i32,
        Field::ScanAdd => edit.working.scan_add() as i32,
        Field::BusyLock => edit.working.busy_lock() as i32,
        Field::AniCall => edit.working.ani_target().map_or(-1, |t| t as i32),
        _ => 0,
    }
}

fn apply(api: &Api, edit: &mut ChannelEdit, field: Field, v: i32) {
    match field {
        Field::Sftd => {
            edit.sftd = v as u8;
            sync_tx_freq(edit);
        }
        Field::RxCts | Field::TxCts => {
            let code = (api.subaudio_code_of_index)(v);
            if field == Field::RxCts {
                edit.working.rx_dcs_cts_num = code;
            } else {
                edit.working.tx_dcs_cts_num = code;
            }
        }
        Field::Power => edit.working.tx_power = v as u8,
        Field::Wn => edit.working.set_wide_narrow(v != 0),
        Field::ScanAdd => edit.working.set_scan_add(v != 0),
        Field::BusyLock => edit.working.set_busy_lock(v != 0),
        Field::AniCall => {
            edit.working.set_ani_target((v >= 0).then_some(v as u8))
        }
        _ => {}
    }
}

fn adjust(api: &Api, edit: &mut ChannelEdit, field: Field, up: bool) {
    let cur = current_value(api, edit, field);
    let new_val = match field {
        Field::Sftd | Field::Power => clamp_step(cur, up, 0, 2),
        Field::RxCts | Field::TxCts => {
            wrap_step(cur, up, -1, SUBAUDIO_MAX_INDEX)
        }
        Field::Wn | Field::ScanAdd | Field::BusyLock => 1 - cur,
        Field::AniCall => clamp_step(cur, up, -1, (CONTACT_COUNT - 1) as i32),
        _ => cur,
    };
    apply(api, edit, field, new_val);
}

fn scalar_floor(field: Field) -> i32 {
    match field {
        Field::RxCts | Field::TxCts | Field::AniCall => -1,
        _ => 0,
    }
}

fn commit_freq_input(edit: &mut ChannelEdit) {
    if !edit.freq_input.is_empty() {
        edit.working.set_rx_freq_deci_hz(edit.freq_input.value());
        sync_tx_freq(edit);
    }
    edit.freq_input.clear();
    edit.editing = false;
}

fn commit_offset_input(edit: &mut ChannelEdit) {
    if !edit.offset_input.is_empty() {
        edit.offset_hz = edit.offset_input.value() * 100;
        sync_tx_freq(edit);
    }
    edit.offset_input.clear();
    edit.editing = false;
}

fn dispatch_freq_edit(ev_kind: u8, ev_key: u8, edit: &mut ChannelEdit) {
    if ev_kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(ev_key) {
        edit.freq_input.push(digit);
        if edit.freq_input.is_full() {
            commit_freq_input(edit);
        }
        return;
    }
    match ev_key {
        KEY_MENU => commit_freq_input(edit),
        KEY_EXIT if edit.freq_input.is_empty() => edit.editing = false,
        KEY_EXIT => edit.freq_input.backspace(),
        _ => {}
    }
}

fn dispatch_offset_edit(ev_kind: u8, ev_key: u8, edit: &mut ChannelEdit) {
    if ev_kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(ev_key) {
        edit.offset_input.push(digit);
        if edit.offset_input.is_full() {
            commit_offset_input(edit);
        }
        return;
    }
    match ev_key {
        KEY_MENU => commit_offset_input(edit),
        KEY_EXIT if edit.offset_input.is_empty() => edit.editing = false,
        KEY_EXIT => edit.offset_input.backspace(),
        _ => {}
    }
}

fn dispatch_name_edit(ev_kind: u8, ev_key: u8, edit: &mut ChannelEdit) {
    if ev_kind == KIND_LONG && ev_key == KEY_EXIT {
        edit.editing = false;
        return;
    }
    if ev_kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(ev_key) {
        edit.name_edit.press_digit(digit);
        return;
    }
    match ev_key {
        KEY_UP => edit.name_edit.move_cursor(true),
        KEY_DOWN => edit.name_edit.move_cursor(false),
        KEY_MENU => {
            edit.name_edit.finalize_pending();
            edit.working.name = edit.name_edit.buf;
            edit.editing = false;
        }
        KEY_EXIT => edit.name_edit.backspace(),
        _ => {}
    }
}

fn format_field_value(
    api: &Api,
    edit: &ChannelEdit,
    index: usize,
    field: Field,
    out: &mut [u8; 18],
) -> bool {
    if edit.is_editing(index) {
        match field {
            Field::Name => {
                for (i, &b) in edit.name_edit.buf.iter().enumerate() {
                    out[i] = if b == 0 || b == 0xFF { b' ' } else { b };
                }
                return true;
            }
            Field::Freq => {
                let n = edit.freq_input.write_display_into(3, out);
                out[n] = 0;
                return true;
            }
            Field::Offset => {
                let n = edit.offset_input.write_display_into(3, out);
                out[n] = 0;
                return true;
            }
            _ => {}
        }
    }
    match field {
        Field::Freq => {
            let hz = edit.working.rx_freq_deci_hz() * 10;
            let n = (api.fmt_freq)(hz, out.as_mut_ptr());
            out[n as usize] = 0;
            true
        }
        Field::Sftd => {
            write_str(
                out,
                match edit.sftd {
                    1 => "+",
                    2 => "-",
                    _ => "OFF",
                },
            );
            true
        }
        Field::Offset => {
            let n = fmt_mhz4(api, edit.offset_hz, out);
            out[n] = 0;
            true
        }
        Field::RxCts | Field::TxCts => {
            let raw = if field == Field::RxCts {
                edit.working.rx_dcs_cts_num
            } else {
                edit.working.tx_dcs_cts_num
            };
            let v = (api.subaudio_index_of_code)(raw);
            let n = (api.subaudio_format)(v, out.as_mut_ptr(), 18);
            out[n as usize] = 0;
            true
        }
        Field::Power => {
            write_str(out, power_label(edit.working.tx_power));
            true
        }
        Field::Wn => {
            write_str(
                out,
                if edit.working.wide_narrow() {
                    "NARROW"
                } else {
                    "WIDE"
                },
            );
            true
        }
        Field::ScanAdd | Field::BusyLock => {
            let on = if field == Field::ScanAdd {
                edit.working.scan_add()
            } else {
                edit.working.busy_lock()
            };
            write_str(out, if on { "ON" } else { "OFF" });
            true
        }
        Field::AniCall => {
            match edit.working.ani_target() {
                Some(t) => {
                    out[0] = b'#';
                    let n = (api.fmt_u32)(t as u32 + 1, out[1..].as_mut_ptr())
                        as usize;
                    out[1 + n] = 0;
                }
                None => write_str(out, "NONE"),
            }
            true
        }
        Field::Name => {
            write_bytes(out, channel_name_bytes(&edit.working.name));
            true
        }
        Field::Save => false,
        Field::Delete => {
            if edit.is_editing(index) {
                write_str(out, "MENU = DELETE");
                true
            } else {
                false
            }
        }
    }
}


/// Closes the editor: the list is told which row to select and which slot to say was saved.
fn back(edit: &ChannelEdit, sel: u16, saved: u16) -> Hand {
    Hand {
        kind: HAND_BACK,
        group: edit.group,
        slot: edit.num,
        new: false,
        sel,
        saved,
    }
}

/// `Some(hand)` when the editor is done and the list should take over.
fn dispatch_detail(
    api: &Api,
    ev_kind: u8,
    ev_key: u8,
    edit: &mut ChannelEdit,
) -> Option<Hand> {
    let field = field_at(edit.field_index);

    if edit.editing {
        match field {
            Field::Name => {
                dispatch_name_edit(ev_kind, ev_key, edit);
                return None;
            }
            Field::Freq => {
                dispatch_freq_edit(ev_kind, ev_key, edit);
                return None;
            }
            Field::Offset => {
                dispatch_offset_edit(ev_kind, ev_key, edit);
                return None;
            }
            _ => {}
        }
    }

    if ev_kind != KIND_SINGLE && ev_kind != KIND_REPEAT {
        return None;
    }

    match ev_key {
        KEY_UP | KEY_DOWN => {
            let up = ev_key == KEY_UP;
            if !edit.editing {
                let n = field_slots(edit);
                edit.field_index = if up {
                    (edit.field_index + n - 1) % n
                } else {
                    (edit.field_index + 1) % n
                };
            } else if field.is_scalar() {
                adjust(api, edit, field, up);
            }
        }
        KEY_DIGIT0
            if ev_kind == KIND_SINGLE && edit.editing && field.is_scalar() =>
        {
            apply(api, edit, field, scalar_floor(field));
        }
        KEY_MENU if ev_kind == KIND_SINGLE => {
            if !edit.editing {
                match field {
                    Field::Save => {
                        write_channel(api, edit.num, &edit.working);
                        return Some(back(edit, edit.num, edit.num));
                    }
                    Field::Delete => edit.editing = true,
                    Field::Name => {
                        edit.name_edit.start(edit.working.name);
                        edit.editing = true;
                    }
                    Field::Freq => {
                        edit.freq_input.clear();
                        edit.editing = true;
                    }
                    Field::Offset => {
                        edit.offset_input.clear();
                        edit.editing = true;
                    }
                    _ => {
                        edit.snapshot = current_value(api, edit, field);
                        edit.editing = true;
                    }
                }
            } else if field == Field::Delete {
                // delete = write an all-0xFF (empty) channel
                let blank = [0xFFu8; CHAN_SIZE];
                let ch = Channel::from_bytes(&blank);
                write_channel(api, edit.num, &ch);
                return Some(back(edit, FIRST_SLOT, NO_SLOT));
            } else {
                edit.editing = false;
            }
        }
        KEY_EXIT if ev_kind == KIND_SINGLE => {
            if edit.editing {
                if field.is_scalar() {
                    let snapshot = edit.snapshot;
                    apply(api, edit, field, snapshot);
                }
                edit.editing = false;
            } else {
                let sel = if edit.is_new { FIRST_SLOT } else { edit.ret_sel };
                return Some(back(edit, sel, NO_SLOT));
            }
        }
        _ => {}
    }

    None
}

static mut EDIT: Option<ChannelEdit> = None;

#[cfg_attr(target_os = "none", no_mangle)]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    unsafe {
        API_PTR = api as *const Api;
    }
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, kind, id),
        AppEvent::Tick { dt_100us } => tick(dt_100us),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => AppResult::Continue,
    }
}

fn enter(api: &Api) -> AppResult {
    unsafe {
        match Hand::take(api, HAND_OPEN) {
            Some(h) => {
                let working = if h.new {
                    default_channel((api.get_master_freq)())
                } else {
                    read_channel(api, h.slot)
                };
                *core::ptr::addr_of_mut!(EDIT) =
                    Some(ChannelEdit::open(h.slot, working, h.new, h.group, h.sel));
                AppResult::Continue
            }
            // opened without the list (nothing to edit): go to the list
            None => {
                Hand {
                    kind: HAND_BACK,
                    group: (api.current_group)(),
                    slot: 0,
                    new: false,
                    sel: FIRST_SLOT,
                    saved: NO_SLOT,
                }
                .put(api);
                AppResult::Chain(SEG_LIST)
            }
        }
    }
}

fn key(api: &Api, kind: u8, id: u8) -> AppResult {
    unsafe {
        if let Some(edit) = (*core::ptr::addr_of_mut!(EDIT)).as_mut() {
            if let Some(h) = dispatch_detail(api, kind, id, edit) {
                h.put(api);
                return AppResult::Chain(SEG_LIST);
            }
        }
    }
    AppResult::Continue
}

fn tick(dt_100us: u32) -> AppResult {
    unsafe {
        if let Some(edit) = (*core::ptr::addr_of_mut!(EDIT)).as_mut() {
            if edit.editing && field_at(edit.field_index) == Field::Name {
                edit.name_edit.tick(dt_100us);
            }
        }
    }
    AppResult::Continue
}

fn draw(api: &Api) -> AppResult {
    static mut ROWS: [ListRow; VISIBLE_ROWS] = [ListRow {
        label: [0; 16],
        value: [0; 18],
        has_value: false,
        cursor: -1,
    }; VISIBLE_ROWS];

    let Some(edit) = (unsafe { (*core::ptr::addr_of!(EDIT)).as_ref() }) else {
        return AppResult::Continue;
    };

    let total = field_slots(edit);
    let selected = edit.field_index;
    let show_arrows = edit.editing && field_at(edit.field_index).is_scalar();
    let window_start = scroll_top(total, selected);

    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        for i in 0..VISIBLE_ROWS {
            let gi = window_start + i;
            rows[i] = ListRow {
                label: [0; 16],
                value: [0; 18],
                has_value: false,
                cursor: -1,
            };
            if gi >= total {
                continue;
            }
            let field = field_at(gi);
            write_str(&mut rows[i].label, field.label());
            rows[i].has_value = format_field_value(
                api,
                edit,
                gi,
                field,
                &mut rows[i].value,
            );
            rows[i].cursor = if edit.is_editing(gi) && field == Field::Name {
                edit.name_edit.cursor as i16
            } else if edit.is_editing(gi) && field == Field::Delete {
                bf5rh_abi::ROW_ARMED
            } else {
                -1
            };
        }
    }

    let mut title_buf = [0u8; TITLE_LEN];
    if edit.is_new {
        write_str(&mut title_buf, "NEW ");
        fmt_ch_num(api, &mut title_buf[4..], edit.group, edit.num);
    } else {
        fmt_ch_num(api, &mut title_buf, edit.group, edit.num);
    }

    // The strip at the bottom says what the keys do in this state.
    let hint: &[u8] = if !edit.editing {
        // leaving the editor without SAVE drops the changes, so EXIT says so
        match field_at(edit.field_index) {
            Field::Save => b"MENU save    EXIT discard",
            Field::Delete => b"MENU delete  EXIT discard",
            _ => b"MENU edit    EXIT discard",
        }
    } else {
        match field_at(edit.field_index) {
            Field::Name => b"0-9 UP/DN MENU ok EXIT del",
            Field::Freq | Field::Offset => b"0-9 type MENU ok EXIT del",
            Field::Delete => b"MENU confirm  EXIT cancel",
            _ => b"UP/DN  MENU ok  EXIT undo",
        }
    };
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);

    let rows_ptr = core::ptr::addr_of!(ROWS) as *const ListRow;
    (api.draw_list)(
        title_buf.as_ptr(),
        TITLE_LEN as u16,
        rows_ptr,
        VISIBLE_ROWS as u16,
        window_start as u16,
        selected as u16,
        total as u16,
        show_arrows,
    );

    AppResult::Continue
}
