#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};

const KIND_SINGLE: u8 = 2;
const KIND_LONG: u8 = 3;
const KIND_REPEAT: u8 = 4;
const KEY_DIGIT0: u8 = 0;
const KEY_STAR: u8 = 10;
const KEY_POUND: u8 = 11;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;

const CHAN_SIZE: usize = 32;
const NAME_SIZE: usize = 12;
const MAX_CHANNEL_NUM: u16 = 999;
const CONTACT_COUNT: usize = 20;
const SUBAUDIO_MAX_INDEX: i32 = 259;
const OCCUPIED_BYTES: usize = 125; // 1000 bits

/// Rows the shared list shows above its key-hint strip.
const VISIBLE_ROWS: usize = 7;
/// Title bytes handed to `draw_list`: a full row of the list font.
const TITLE_LEN: usize = 26;

static mut API_PTR: *const Api = core::ptr::null();

fn bcd_byte_to_decimal(b: u8) -> u32 {
    ((b >> 4) * 10 + (b & 0x0f)) as u32
}

fn decimal_to_bcd_byte(v: u32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

fn bcd4_to_deci_hz(bcd: [u8; 4]) -> u32 {
    bcd_byte_to_decimal(bcd[3]) * 1_000_000
        + bcd_byte_to_decimal(bcd[2]) * 10_000
        + bcd_byte_to_decimal(bcd[1]) * 100
        + bcd_byte_to_decimal(bcd[0])
}

fn deci_hz_to_bcd4(deci_hz: u32) -> [u8; 4] {
    [
        decimal_to_bcd_byte(deci_hz % 100),
        decimal_to_bcd_byte((deci_hz / 100) % 100),
        decimal_to_bcd_byte((deci_hz / 10_000) % 100),
        decimal_to_bcd_byte(deci_hz / 1_000_000),
    ]
}

#[derive(Clone, Copy)]
struct Channel {
    rx_freq_bcd: [u8; 4],
    tx_freq_bcd: [u8; 4],
    rx_dcs_cts_num: u16,
    tx_dcs_cts_num: u16,
    dtmf_group: u8,
    ptt_id: u8,
    tx_power: u8,
    flags: u8,
    decoder_code: u32,
    name: [u8; NAME_SIZE],
}

impl Channel {
    fn from_bytes(buf: &[u8; CHAN_SIZE]) -> Channel {
        Channel {
            rx_freq_bcd: [buf[0], buf[1], buf[2], buf[3]],
            tx_freq_bcd: [buf[4], buf[5], buf[6], buf[7]],
            rx_dcs_cts_num: u16::from_le_bytes([buf[8], buf[9]]),
            tx_dcs_cts_num: u16::from_le_bytes([buf[10], buf[11]]),
            dtmf_group: buf[12],
            ptt_id: buf[13],
            tx_power: buf[14],
            flags: buf[15],
            decoder_code: u32::from_le_bytes([
                buf[16], buf[17], buf[18], buf[19],
            ]),
            name: {
                let mut n = [0u8; NAME_SIZE];
                n.copy_from_slice(&buf[20..32]);
                n
            },
        }
    }

    fn to_bytes(self) -> [u8; CHAN_SIZE] {
        let mut buf = [0u8; CHAN_SIZE];
        buf[0..4].copy_from_slice(&self.rx_freq_bcd);
        buf[4..8].copy_from_slice(&self.tx_freq_bcd);
        buf[8..10].copy_from_slice(&self.rx_dcs_cts_num.to_le_bytes());
        buf[10..12].copy_from_slice(&self.tx_dcs_cts_num.to_le_bytes());
        buf[12] = self.dtmf_group;
        buf[13] = self.ptt_id;
        buf[14] = self.tx_power;
        buf[15] = self.flags;
        buf[16..20].copy_from_slice(&self.decoder_code.to_le_bytes());
        buf[20..32].copy_from_slice(&self.name);
        buf
    }

    fn rx_freq_deci_hz(&self) -> u32 {
        bcd4_to_deci_hz(self.rx_freq_bcd)
    }
    fn set_rx_freq_deci_hz(&mut self, deci_hz: u32) {
        self.rx_freq_bcd = deci_hz_to_bcd4(deci_hz);
    }
    fn tx_freq_deci_hz(&self) -> u32 {
        bcd4_to_deci_hz(self.tx_freq_bcd)
    }
    fn set_tx_freq_deci_hz(&mut self, deci_hz: u32) {
        self.tx_freq_bcd = deci_hz_to_bcd4(deci_hz);
    }

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

fn channel_name_bytes(raw: &[u8; NAME_SIZE]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0x00 || b == 0xFF)
        .unwrap_or(raw.len());
    &raw[..end]
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

struct ChanList {
    /// Stored channels that belong to the group being shown: what the list displays. (One map only: the app has
    /// little memory to spare, so a free slot is found by asking the radio when a channel is created.)
    member: [u8; OCCUPIED_BYTES],
    selected: Option<u16>,
}

impl ChanList {
    const fn blank() -> Self {
        ChanList {
            member: [0; OCCUPIED_BYTES],
            selected: None,
        }
    }

    fn scan(api: &Api) -> Self {
        let group = shown_group();
        let mut member = [0u8; OCCUPIED_BYTES];
        let mut buf = [0u8; CHAN_SIZE];
        for n in 0..=MAX_CHANNEL_NUM {
            if (api.chan_in_group)(group, n)
                && (api.chan_read)(n, buf.as_mut_ptr(), CHAN_SIZE as u16)
            {
                member[n as usize / 8] |= 1 << (n % 8);
            }
        }
        let selected = (0..=MAX_CHANNEL_NUM).find(|&n| Self::bit(&member, n));
        ChanList { member, selected }
    }

    fn bit(occupied: &[u8; OCCUPIED_BYTES], n: u16) -> bool {
        occupied[n as usize / 8] & (1 << (n % 8)) != 0
    }

    fn count(&self) -> u16 {
        self.member.iter().map(|b| b.count_ones() as u16).sum()
    }

    fn step(&self, down: bool) -> Option<u16> {
        match self.selected {
            Some(num) if down => ((num + 1)..=MAX_CHANNEL_NUM)
                .find(|&n| Self::bit(&self.member, n)),
            Some(num) => (0..num).rev().find(|&n| Self::bit(&self.member, n)),
            None if down => {
                (0..=MAX_CHANNEL_NUM).find(|&n| Self::bit(&self.member, n))
            }
            None => (0..=MAX_CHANNEL_NUM)
                .rev()
                .find(|&n| Self::bit(&self.member, n)),
        }
    }

    fn selected_index(&self) -> usize {
        match self.selected {
            Some(num) => {
                (0..num).filter(|&n| Self::bit(&self.member, n)).count()
            }
            None => self.count() as usize,
        }
    }

    fn channel_at(&self, index: usize) -> Option<u16> {
        if index >= self.count() as usize {
            return None;
        }
        (0..=MAX_CHANNEL_NUM)
            .filter(|&n| Self::bit(&self.member, n))
            .nth(index)
    }
}

fn rescan_list(api: &Api, prefer: Option<u16>) -> ChanList {
    let mut list = ChanList::scan(api);
    if let Some(num) = prefer {
        if ChanList::bit(&list.member, num) {
            list.selected = Some(num);
        }
    }
    list
}

struct ChannelEdit {
    num: u16,
    is_new: bool,
    working: Channel,
    sftd: u8,
    offset_hz: u32,
    field_index: usize,
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
    fn open(num: u16, working: Channel, is_new: bool) -> Self {
        let (sftd, offset_hz) = derive_shift(&working);
        ChannelEdit {
            num,
            is_new,
            working,
            sftd,
            offset_hz,
            field_index: 0,
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

fn dispatch_list(
    api: &Api,
    ev_kind: u8,
    ev_key: u8,
    mut list: ChanList,
) -> Phase {
    if ev_kind != KIND_SINGLE && ev_kind != KIND_REPEAT {
        return Phase::List(list);
    }
    match ev_key {
        KEY_UP => {
            list.selected = list.step(false);
            Phase::List(list)
        }
        KEY_DOWN => {
            list.selected = list.step(true);
            Phase::List(list)
        }
        KEY_MENU if ev_kind == KIND_SINGLE => match list.selected {
            Some(num) => {
                let working = read_channel(api, num);
                Phase::Detail(ChannelEdit::open(num, working, false))
            }
            // a new channel goes into the first free slot that belongs to the group being shown
            None => match (0..=MAX_CHANNEL_NUM).find(|&n| {
                let mut buf = [0u8; CHAN_SIZE];
                (api.chan_in_group)(shown_group(), n)
                    && !(api.chan_read)(n, buf.as_mut_ptr(), CHAN_SIZE as u16)
            }) {
                Some(num) => {
                    let working = default_channel((api.get_master_freq)());
                    Phase::Detail(ChannelEdit::open(num, working, true))
                }
                None => Phase::List(list),
            },
        },
        KEY_EXIT if ev_kind == KIND_SINGLE => Phase::Exit,
        KEY_STAR | KEY_POUND if ev_kind == KIND_SINGLE => {
            step_group(ev_key == KEY_POUND);
            Phase::List(ChanList::scan(api))
        }
        _ => Phase::List(list),
    }
}

fn dispatch_detail(
    api: &Api,
    ev_kind: u8,
    ev_key: u8,
    mut edit: ChannelEdit,
) -> Phase {
    let field = field_at(edit.field_index);

    if edit.editing {
        match field {
            Field::Name => {
                dispatch_name_edit(ev_kind, ev_key, &mut edit);
                return Phase::Detail(edit);
            }
            Field::Freq => {
                dispatch_freq_edit(ev_kind, ev_key, &mut edit);
                return Phase::Detail(edit);
            }
            Field::Offset => {
                dispatch_offset_edit(ev_kind, ev_key, &mut edit);
                return Phase::Detail(edit);
            }
            _ => {}
        }
    }

    if ev_kind != KIND_SINGLE && ev_kind != KIND_REPEAT {
        return Phase::Detail(edit);
    }

    match ev_key {
        KEY_UP | KEY_DOWN => {
            let up = ev_key == KEY_UP;
            if !edit.editing {
                let n = field_slots(&edit);
                edit.field_index = if up {
                    (edit.field_index + n - 1) % n
                } else {
                    (edit.field_index + 1) % n
                };
            } else if field.is_scalar() {
                adjust(api, &mut edit, field, up);
            }
        }
        KEY_DIGIT0
            if ev_kind == KIND_SINGLE && edit.editing && field.is_scalar() =>
        {
            apply(api, &mut edit, field, scalar_floor(field));
        }
        KEY_MENU if ev_kind == KIND_SINGLE => {
            if !edit.editing {
                match field {
                    Field::Save => {
                        write_channel(api, edit.num, &edit.working);
                        return Phase::List(rescan_list(api, Some(edit.num)));
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
                        edit.snapshot = current_value(api, &edit, field);
                        edit.editing = true;
                    }
                }
            } else if field == Field::Delete {
                // 删除 = 写全 0xFF（空信道）
                let blank = [0xFFu8; CHAN_SIZE];
                let ch = Channel::from_bytes(&blank);
                write_channel(api, edit.num, &ch);
                return Phase::List(rescan_list(api, None));
            } else {
                edit.editing = false;
            }
        }
        KEY_EXIT if ev_kind == KIND_SINGLE => {
            if edit.editing {
                if field.is_scalar() {
                    let snapshot = edit.snapshot;
                    apply(api, &mut edit, field, snapshot);
                }
                edit.editing = false;
            } else {
                let prefer = (!edit.is_new).then_some(edit.num);
                return Phase::List(rescan_list(api, prefer));
            }
        }
        _ => {}
    }

    Phase::Detail(edit)
}

enum Phase {
    List(ChanList),
    Detail(ChannelEdit),
    Exit,
}

fn read_channel(api: &Api, num: u16) -> Channel {
    let mut buf = [0u8; CHAN_SIZE];
    if (api.chan_read)(num, buf.as_mut_ptr(), CHAN_SIZE as u16) {
        Channel::from_bytes(&buf)
    } else {
        Channel::from_bytes(&[0xFF; CHAN_SIZE])
    }
}

fn write_channel(api: &Api, num: u16, ch: &Channel) {
    let _ = (api.chan_write)(num, ch.to_bytes().as_ptr(), CHAN_SIZE as u16);
}

struct State {
    phase: Phase,
    /// Slot just written by SAVE: a toast says so until a key is pressed or `TOAST_100US` passes.
    saved: Option<u16>,
    saved_age_100us: u32,
    /// The channel group the list shows (starts as the radio's current group; `*` and `#` step through them).
    group: u8,
    group_count: u8,
}

static mut STATE: State = State {
    phase: Phase::List(ChanList::blank()),
    saved: None,
    saved_age_100us: 0,
    group: 0,
    group_count: 1,
};

const TOAST_100US: u32 = 15_000;
const GROUP_NAME_MAX: usize = 16;

fn shown_group() -> u8 {
    unsafe { (*core::ptr::addr_of!(STATE)).group }
}

fn step_group(next: bool) {
    unsafe {
        let st = &mut *core::ptr::addr_of_mut!(STATE);
        let n = st.group_count.max(1);
        st.group = if next { (st.group + 1) % n } else { (st.group + n - 1) % n };
    }
}

fn dismiss_toast(api: &Api, state: &mut State) {
    if state.saved.take().is_some() {
        (api.toast)(core::ptr::null(), 0);
    }
}

/// "GROUP 1: REPEATERS": the number is the digit that selects the group from the BAND menu.
fn group_title(api: &Api, out: &mut [u8; TITLE_LEN]) {
    let group = shown_group();
    *out = [0; TITLE_LEN];
    out[..6].copy_from_slice(b"GROUP ");
    let mut i = 6 + (api.fmt_u32)(group as u32, out[6..].as_mut_ptr()) as usize;
    out[i] = b':';
    out[i + 1] = b' ';
    i += 2;
    (api.group_name)(group, out[i..].as_mut_ptr(), (TITLE_LEN - i) as u16);
}

#[cfg_attr(target_os = "none", no_mangle)]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    unsafe {
        API_PTR = api as *const Api;
    }
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, kind, id),
        AppEvent::Tick { dt_100us } => tick(api, dt_100us),
        AppEvent::Draw => draw(api),
        AppEvent::Leave => AppResult::Continue,
    }
}

fn enter(api: &Api) -> AppResult {
    unsafe {
        let st = &mut *core::ptr::addr_of_mut!(STATE);
        let mut scratch = [0u8; GROUP_NAME_MAX];
        let mut count = 0u8;
        while count < 64 && (api.group_name)(count, scratch.as_mut_ptr(), GROUP_NAME_MAX as u16) != 0 {
            count += 1;
        }
        st.group_count = count.max(1);
        st.group = (api.current_group)().min(st.group_count - 1);
        st.saved = None;
        st.phase = Phase::List(ChanList::scan(api));
    }
    AppResult::Continue
}

fn key(api: &Api, kind: u8, id: u8) -> AppResult {
    unsafe {
        let state = &mut *core::ptr::addr_of_mut!(STATE);
        let phase = core::mem::replace(
            &mut state.phase,
            Phase::List(ChanList::blank()),
        );
        dismiss_toast(api, state);
        state.phase = match phase {
            Phase::List(list) => dispatch_list(api, kind, id, list),
            Phase::Detail(edit) => {
                let saving = kind == KIND_SINGLE
                    && id == KEY_MENU
                    && !edit.editing
                    && field_at(edit.field_index) == Field::Save;
                let num = edit.num;
                let next = dispatch_detail(api, kind, id, edit);
                if saving && matches!(next, Phase::List(_)) {
                    state.saved = Some(num);
                    state.saved_age_100us = 0;
                }
                next
            }
            Phase::Exit => Phase::Exit,
        };
        if matches!(state.phase, Phase::Exit) {
            return AppResult::Exit;
        }
    }
    AppResult::Continue
}

fn tick(api: &Api, dt_100us: u32) -> AppResult {
    unsafe {
        let state = &mut *core::ptr::addr_of_mut!(STATE);
        if state.saved.is_some() {
            state.saved_age_100us = state.saved_age_100us.saturating_add(dt_100us);
            if state.saved_age_100us >= TOAST_100US {
                dismiss_toast(api, state);
            }
        }
        if let Phase::Detail(edit) = &mut state.phase {
            if edit.editing && field_at(edit.field_index) == Field::Name {
                edit.name_edit.tick(dt_100us);
            }
        }
    }
    AppResult::Continue
}

fn scroll_top(total: usize, selected: usize) -> usize {
    if total <= VISIBLE_ROWS {
        0
    } else {
        selected
            .saturating_sub(VISIBLE_ROWS / 2)
            .min(total - VISIBLE_ROWS)
    }
}

/// 裸字节版（不经 `&str`，见 `channel_name_bytes` 的理由）。
fn write_bytes(out: &mut [u8], s: &[u8]) {
    let n = s.len().min(out.len());
    out[..n].copy_from_slice(&s[..n]);
    for b in out[n..].iter_mut() {
        *b = 0;
    }
}

fn write_str(out: &mut [u8], s: &str) {
    let n = s.len().min(out.len());
    out[..n].copy_from_slice(&s.as_bytes()[..n]);
    for b in out[n..].iter_mut() {
        *b = 0;
    }
}

fn fmt_mhz4(api: &Api, hz: u32, out: &mut [u8]) -> usize {
    let mhz = hz / 1_000_000;
    let mut frac = (hz % 1_000_000) / 100;
    let mut idx = 0;
    idx += (api.fmt_u32)(mhz, out.as_mut_ptr()) as usize;
    out[idx] = b'.';
    idx += 1;
    let mut fbuf = [b'0'; 4];
    for i in (0..4).rev() {
        fbuf[i] = (frac % 10) as u8 + b'0';
        frac /= 10;
    }
    out[idx..idx + 4].copy_from_slice(&fbuf);
    idx + 4
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

/// The number the main screen shows for slot `num`: its position in the group being shown, at least two digits
/// ("01"; in ALL the printed channel number). Returns the length written.
fn fmt_num(api: &Api, out: &mut [u8], num: u16) -> usize {
    let pos = (api.chan_group_pos)(shown_group(), num) as u32;
    let mut i = 0;
    if pos < 10 {
        out[0] = b'0';
        i = 1;
    }
    i + (api.fmt_u32)(pos, out[i..].as_mut_ptr()) as usize
}

/// "CH" and the number; returns the length written.
fn fmt_ch_num(api: &Api, out: &mut [u8], num: u16) -> usize {
    out[0] = b'C';
    out[1] = b'H';
    2 + fmt_num(api, &mut out[2..], num)
}

fn draw(api: &Api) -> AppResult {
    static mut ROWS: [ListRow; VISIBLE_ROWS] = [ListRow {
        label: [0; 16],
        value: [0; 18],
        has_value: false,
        cursor: -1,
    }; VISIBLE_ROWS];

    let state = unsafe { &*core::ptr::addr_of!(STATE) };

    let (total, selected, show_arrows) = match &state.phase {
        Phase::List(list) => {
            (list.count() as usize + 1, list.selected_index(), false)
        }
        Phase::Detail(edit) => (
            field_slots(edit),
            edit.field_index,
            edit.editing && field_at(edit.field_index).is_scalar(),
        ),
        Phase::Exit => return AppResult::Continue,
    };

    let window_start = scroll_top(total, selected);

    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        match &state.phase {
            Phase::List(list) => {
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
                    match list.channel_at(gi) {
                        Some(num) => {
                            // "01 K5RWK       147.1200": the number the main screen shows, the name, the frequency
                            let ch = read_channel(api, num);
                            let n = fmt_num(api, &mut rows[i].label, num);
                            rows[i].label[n] = b' ';
                            write_bytes(&mut rows[i].label[n + 1..], channel_name_bytes(&ch.name));
                            fmt_mhz4(api, ch.rx_freq_deci_hz() * 10, &mut rows[i].value);
                            rows[i].has_value = true;
                        }
                        None => write_str(&mut rows[i].label, "+ NEW"),
                    }
                }
            }
            Phase::Detail(edit) => {
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
                    rows[i].cursor =
                        if edit.is_editing(gi) && field == Field::Name {
                            edit.name_edit.cursor as i16
                        } else if edit.is_editing(gi) && field == Field::Delete {
                            bf5rh_abi::ROW_ARMED
                        } else {
                            -1
                        };
                }
            }
            Phase::Exit => {}
        }
    }

    let mut title_buf = [0u8; TITLE_LEN];
    match &state.phase {
        Phase::List(_) => group_title(api, &mut title_buf),
        Phase::Detail(edit) => {
            if edit.is_new {
                write_str(&mut title_buf, "NEW ");
                fmt_ch_num(api, &mut title_buf[4..], edit.num);
            } else {
                fmt_ch_num(api, &mut title_buf, edit.num);
            }
        }
        Phase::Exit => {}
    }

    // The strip at the bottom says what the keys do in this state.
    let hint: &[u8] = match &state.phase {
        Phase::List(_) => b"MENU open   * # group",
        // leaving the editor without SAVE drops the changes, so EXIT says so
        Phase::Detail(edit) if !edit.editing => match field_at(edit.field_index) {
            Field::Save => b"MENU save    EXIT discard",
            Field::Delete => b"MENU delete  EXIT discard",
            _ => b"MENU edit    EXIT discard",
        },
        Phase::Detail(edit) => match field_at(edit.field_index) {
            Field::Name => b"0-9 UP/DN MENU ok EXIT del",
            Field::Freq | Field::Offset => b"0-9 type MENU ok EXIT del",
            Field::Delete => b"MENU confirm  EXIT cancel",
            _ => b"UP/DN  MENU ok  EXIT undo",
        },
        Phase::Exit => b"",
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
    if let Some(num) = state.saved {
        let mut text = [0u8; 16];
        text[..6].copy_from_slice(b"SAVED ");
        let n = 6 + fmt_ch_num(api, &mut text[6..], num);
        (api.toast)(text.as_ptr(), n as u16);
    }

    AppResult::Continue
}

#[cfg(target_os = "none")]
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
