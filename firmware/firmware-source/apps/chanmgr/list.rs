//! CHAN MGR segment 0: the channel list of one group. MENU on a row (or on "+ NEW") chains to the editor segment.
use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};

use super::common::*;

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

    fn scan(api: &Api, group: u8) -> Self {
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

struct State {
    list: ChanList,
    /// Slot just written by SAVE: a toast says so until a key is pressed or `TOAST_100US` passes.
    saved: Option<u16>,
    saved_age_100us: u32,
    /// The channel group the list shows (starts as the radio's current group; `*` and `#` step through them).
    group: u8,
    group_count: u8,
}

static mut STATE: State = State {
    list: ChanList::blank(),
    saved: None,
    saved_age_100us: 0,
    group: 0,
    group_count: 1,
};

const TOAST_100US: u32 = 15_000;

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
fn group_title(api: &Api, group: u8, out: &mut [u8; TITLE_LEN]) {
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
        // Back from the editor: the same group, the row it was opened from (or the one it saved or deleted).
        let back = Hand::take(api, HAND_BACK);
        if let Some(h) = back {
            st.group = h.group.min(st.group_count - 1);
        }
        st.list = ChanList::scan(api, st.group);
        if let Some(h) = back {
            match h.sel {
                NO_SLOT => st.list.selected = None,
                FIRST_SLOT => {}
                n if ChanList::bit(&st.list.member, n) => st.list.selected = Some(n),
                _ => {}
            }
            if h.saved != NO_SLOT {
                st.saved = Some(h.saved);
                st.saved_age_100us = 0;
            }
        }
    }
    AppResult::Continue
}

/// Hands over to the editor for slot `num`.
fn open_editor(api: &Api, state: &State, num: u16, new: bool) -> AppResult {
    Hand {
        kind: HAND_OPEN,
        group: state.group,
        slot: num,
        new,
        sel: state.list.selected.unwrap_or(NO_SLOT),
        saved: NO_SLOT,
    }
    .put(api);
    AppResult::Chain(SEG_EDIT)
}

fn key(api: &Api, kind: u8, id: u8) -> AppResult {
    unsafe {
        let state = &mut *core::ptr::addr_of_mut!(STATE);
        // The core also sends Press (0) and Release (1) around every Single (device/keypad.rs). Dismissing the notice
        // on them removed SAVED CHnn on the Release that follows the Single that raised it, before it was ever drawn
        // (found by making the simulator send them: docs/TEST-DOUBLES.md).
        if kind < KIND_SINGLE {
            return AppResult::Continue;
        }
        dismiss_toast(api, state);
        if kind != KIND_SINGLE && kind != KIND_REPEAT {
            return AppResult::Continue;
        }
        match id {
            KEY_UP => state.list.selected = state.list.step(false),
            KEY_DOWN => state.list.selected = state.list.step(true),
            KEY_MENU if kind == KIND_SINGLE => match state.list.selected {
                Some(num) => return open_editor(api, state, num, false),
                // a new channel goes into the first free slot that belongs to the group being shown
                None => {
                    let group = state.group;
                    let free = (0..=MAX_CHANNEL_NUM).find(|&n| {
                        let mut buf = [0u8; CHAN_SIZE];
                        (api.chan_in_group)(group, n)
                            && !(api.chan_read)(n, buf.as_mut_ptr(), CHAN_SIZE as u16)
                    });
                    if let Some(num) = free {
                        return open_editor(api, state, num, true);
                    }
                }
            },
            KEY_EXIT if kind == KIND_SINGLE => return AppResult::Exit,
            KEY_STAR | KEY_POUND if kind == KIND_SINGLE => {
                step_group(id == KEY_POUND);
                state.list = ChanList::scan(api, state.group);
            }
            _ => {}
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
    }
    AppResult::Continue
}

fn draw(api: &Api) -> AppResult {
    // on the stack, not in the arena (drawing is synchronous: the core is done with the rows when `draw_list` returns)
    let mut rows = [ListRow {
        label: [0; 16],
        value: [0; 18],
        has_value: false,
        cursor: -1,
    }; VISIBLE_ROWS];

    let state = unsafe { &*core::ptr::addr_of!(STATE) };
    let list = &state.list;
    let total = list.count() as usize + 1;
    let selected = list.selected_index();
    let window_start = scroll_top(total, selected);

    {
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
                    let n = fmt_num(api, &mut rows[i].label, state.group, num);
                    rows[i].label[n] = b' ';
                    write_bytes(&mut rows[i].label[n + 1..], channel_name_bytes(&ch.name));
                    fmt_mhz4(api, ch.rx_freq_deci_hz() * 10, &mut rows[i].value);
                    rows[i].has_value = true;
                }
                None => write_str(&mut rows[i].label, "+ NEW"),
            }
        }
    }

    let mut title_buf = [0u8; TITLE_LEN];
    group_title(api, state.group, &mut title_buf);

    // The strip at the bottom says what the keys do in this state.
    let hint: &[u8] = b"MENU open   * # group";
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);

    let rows_ptr = rows.as_ptr();
    (api.draw_list)(
        title_buf.as_ptr(),
        TITLE_LEN as u16,
        rows_ptr,
        VISIBLE_ROWS as u16,
        window_start as u16,
        selected as u16,
        total as u16,
        false,
    );
    if let Some(num) = state.saved {
        let mut text = [0u8; 16];
        text[..6].copy_from_slice(b"SAVED ");
        let n = 6 + fmt_num(api, &mut text[6..], state.group, num);
        (api.toast)(text.as_ptr(), n as u16);
    }

    AppResult::Continue
}
