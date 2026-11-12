//! CHAN MGR code shared by both segments (list and editor): the channel record, number formatting and the handoff.
//!
//! The package is two segments because one did not leave room to add anything: `list` shows the channels of a group,
//! `edit` edits one. They switch with `AppResult::Chain` and talk through the 64-byte handoff buffer (`Hand`).
#![allow(dead_code)]

use bf5rh_abi::Api;

pub const KIND_SINGLE: u8 = 2;
pub const KIND_LONG: u8 = 3;
pub const KIND_REPEAT: u8 = 4;
pub const KEY_DIGIT0: u8 = 0;
pub const KEY_STAR: u8 = 10;
pub const KEY_POUND: u8 = 11;
pub const KEY_MENU: u8 = 12;
pub const KEY_EXIT: u8 = 13;
pub const KEY_UP: u8 = 14;
pub const KEY_DOWN: u8 = 15;

pub const SEG_LIST: u8 = 0;
pub const SEG_EDIT: u8 = 1;

pub const CHAN_SIZE: usize = 32;
pub const NAME_SIZE: usize = 12;
pub const MAX_CHANNEL_NUM: u16 = 999;
pub const CONTACT_COUNT: usize = 20;
pub const SUBAUDIO_MAX_INDEX: i32 = 259;
pub const OCCUPIED_BYTES: usize = 125; // 1000 bits

/// Rows the shared list shows above its key-hint strip.
pub const VISIBLE_ROWS: usize = 7;
/// Title bytes handed to `draw_list`: a full row of the list font.
pub const TITLE_LEN: usize = 26;
pub const GROUP_NAME_MAX: usize = 16;

pub static mut API_PTR: *const Api = core::ptr::null();

// handoff: what the segments tell each other

const HAND_MAGIC: u8 = 0xC7;
/// list -> editor: open `slot` (`new` when it is a free slot to create)
pub const HAND_OPEN: u8 = 0;
/// editor -> list: back to the list, selecting `sel`, and say `saved` was written
pub const HAND_BACK: u8 = 1;
/// no slot (the "+ NEW" row in `sel`, nothing saved in `saved`)
pub const NO_SLOT: u16 = 0xFFFF;
/// `sel` meaning "the first channel of the group" (what a fresh list shows)
pub const FIRST_SLOT: u16 = 0xFFFE;

/// `group` is the group the list was showing and `sel` the row it had selected, so EXIT from the editor returns to
/// that row: the list scrolls by the selected row alone, so those two fix what it shows.
#[derive(Clone, Copy)]
pub struct Hand {
    pub kind: u8,
    pub group: u8,
    /// HAND_OPEN: the slot to edit
    pub slot: u16,
    pub new: bool,
    pub sel: u16,
    pub saved: u16,
}

impl Hand {
    pub fn put(&self, api: &Api) {
        let mut b = [0u8; 10];
        b[0] = HAND_MAGIC;
        b[1] = self.kind;
        b[2] = self.group;
        b[3..5].copy_from_slice(&self.slot.to_le_bytes());
        b[5] = self.new as u8;
        b[6..8].copy_from_slice(&self.sel.to_le_bytes());
        b[8..10].copy_from_slice(&self.saved.to_le_bytes());
        let _ = (api.handoff_write)(b.as_ptr(), b.len() as u16);
    }

    /// The pending handoff if it is of `kind` (a freshly opened app has none).
    pub fn take(api: &Api, kind: u8) -> Option<Hand> {
        let mut b = [0u8; 10];
        let n = (api.handoff_read)(b.as_mut_ptr(), b.len() as u16);
        if n < 10 || b[0] != HAND_MAGIC || b[1] != kind {
            return None;
        }
        Some(Hand {
            kind,
            group: b[2],
            slot: u16::from_le_bytes([b[3], b[4]]),
            new: b[5] != 0,
            sel: u16::from_le_bytes([b[6], b[7]]),
            saved: u16::from_le_bytes([b[8], b[9]]),
        })
    }
}

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
pub struct Channel {
    pub rx_freq_bcd: [u8; 4],
    pub tx_freq_bcd: [u8; 4],
    pub rx_dcs_cts_num: u16,
    pub tx_dcs_cts_num: u16,
    pub dtmf_group: u8,
    pub ptt_id: u8,
    pub tx_power: u8,
    pub flags: u8,
    pub decoder_code: u32,
    pub name: [u8; NAME_SIZE],
}

impl Channel {
    pub fn from_bytes(buf: &[u8; CHAN_SIZE]) -> Channel {
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

    pub fn to_bytes(self) -> [u8; CHAN_SIZE] {
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

    pub fn rx_freq_deci_hz(&self) -> u32 {
        bcd4_to_deci_hz(self.rx_freq_bcd)
    }
    pub fn set_rx_freq_deci_hz(&mut self, deci_hz: u32) {
        self.rx_freq_bcd = deci_hz_to_bcd4(deci_hz);
    }
    pub fn tx_freq_deci_hz(&self) -> u32 {
        bcd4_to_deci_hz(self.tx_freq_bcd)
    }
    pub fn set_tx_freq_deci_hz(&mut self, deci_hz: u32) {
        self.tx_freq_bcd = deci_hz_to_bcd4(deci_hz);
    }
}

pub fn channel_name_bytes(raw: &[u8; NAME_SIZE]) -> &[u8] {
    let end = raw
        .iter()
        .position(|&b| b == 0x00 || b == 0xFF)
        .unwrap_or(raw.len());
    &raw[..end]
}

pub fn read_channel(api: &Api, num: u16) -> Channel {
    let mut buf = [0u8; CHAN_SIZE];
    if (api.chan_read)(num, buf.as_mut_ptr(), CHAN_SIZE as u16) {
        Channel::from_bytes(&buf)
    } else {
        Channel::from_bytes(&[0xFF; CHAN_SIZE])
    }
}

pub fn write_channel(api: &Api, num: u16, ch: &Channel) {
    let _ = (api.chan_write)(num, ch.to_bytes().as_ptr(), CHAN_SIZE as u16);
}

/// Raw-bytes version (does not go through `&str`; see `channel_name_bytes`).
pub fn write_bytes(out: &mut [u8], s: &[u8]) {
    let n = s.len().min(out.len());
    out[..n].copy_from_slice(&s[..n]);
    for b in out[n..].iter_mut() {
        *b = 0;
    }
}

pub fn write_str(out: &mut [u8], s: &str) {
    let n = s.len().min(out.len());
    out[..n].copy_from_slice(&s.as_bytes()[..n]);
    for b in out[n..].iter_mut() {
        *b = 0;
    }
}

pub fn fmt_mhz4(api: &Api, hz: u32, out: &mut [u8]) -> usize {
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

/// The number the main screen shows for slot `num`: its position in `group`, at least two digits ("01"; in ALL the
/// printed channel number). Returns the length written.
pub fn fmt_num(api: &Api, out: &mut [u8], group: u8, num: u16) -> usize {
    let pos = (api.chan_group_pos)(group, num) as u32;
    let mut i = 0;
    if pos < 10 {
        out[0] = b'0';
        i = 1;
    }
    i + (api.fmt_u32)(pos, out[i..].as_mut_ptr()) as usize
}

/// "CH" and the number; returns the length written.
pub fn fmt_ch_num(api: &Api, out: &mut [u8], group: u8, num: u16) -> usize {
    out[0] = b'C';
    out[1] = b'H';
    2 + fmt_num(api, &mut out[2..], group, num)
}

pub fn scroll_top(total: usize, selected: usize) -> usize {
    if total <= VISIBLE_ROWS {
        0
    } else {
        selected
            .saturating_sub(VISIBLE_ROWS / 2)
            .min(total - VISIBLE_ROWS)
    }
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
