#![allow(dead_code)]

use bf5rh_abi::{Api, AppEvent, AppResult, ListRow};

use crate::MY_SEG;

pub const KIND_SINGLE: u8 = 2;
pub const KIND_LONG: u8 = 3;
pub const KIND_REPEAT: u8 = 4;
pub const KEY_DIGIT0: u8 = 0;
pub const KEY_ASTERISK: u8 = 10;
pub const KEY_POUND: u8 = 11;
pub const KEY_MENU: u8 = 12;
pub const KEY_EXIT: u8 = 13;
pub const KEY_UP: u8 = 14;
pub const KEY_DOWN: u8 = 15;

pub const SETTINGS_BYTES: usize = 168;
pub const SIDE_CFG_BYTES: usize = 14;
/// Rows the shared list shows above its key-hint strip.
pub const VISIBLE_ROWS: usize = 7;

pub const COORD_NOT_SET: i32 = i32::MAX;
pub const RIT_STEP_HZ: i32 = 10;
pub const BATTERY_CAL_REFERENCE_CV: u32 = 760;
pub const STEP_LIST_DECI_HZ: [u32; 9] =
    [250, 500, 625, 1000, 1250, 2000, 2500, 5000, 10000];
pub const RTONE_HZ_DIV_10: [u16; 4] = [100, 145, 175, 210];
pub const CONTACT_COUNT: i32 = 20;
pub const SUBAUDIO_MAX_INDEX: i32 = 259;

// segments

pub const SEG_MENU: u8 = 0;
pub const SEG_RADIO: u8 = 1;
pub const SEG_SYSTEM: u8 = 2;

pub const GROUP_SEG: [u8; 6] = [
    SEG_RADIO,  // Radio
    SEG_MENU,   // Sig
    SEG_MENU,   // Display
    SEG_MENU,   // Keys
    SEG_MENU,   // Ani
    SEG_SYSTEM, // System
];

/// `true` when this segment compiled the items of group `gi`.
const fn have_group(gi: usize) -> bool {
    match gi {
        0 => cfg!(feature = "g_radio"),
        1 => cfg!(feature = "g_audio"),
        2 => cfg!(feature = "g_disp"),
        3 => cfg!(feature = "g_keys"),
        4 => cfg!(feature = "g_ani"),
        _ => cfg!(feature = "g_system"),
    }
}

pub const fn assert_seg_features() {
    let mut gi = 0;
    while gi < GROUP_SEG.len() {
        assert!(
            have_group(gi) == (GROUP_SEG[gi] == MY_SEG),
            "segment feature list disagrees with GROUP_SEG"
        );
        gi += 1;
    }
}

// handoff

const HANDOFF_MAGIC: u8 = 0xA5;
const ACT_GROUPS: u8 = 0;
const ACT_ITEMS: u8 = 1;

fn handoff_put(api: &Api, group: usize, action: u8) {
    let buf = [HANDOFF_MAGIC, group as u8, action];
    let _ = (api.handoff_write)(buf.as_ptr(), buf.len() as u16);
}

fn handoff_take(api: &Api) -> Option<(usize, u8)> {
    let mut buf = [0u8; 3];
    let n = (api.handoff_read)(buf.as_mut_ptr(), buf.len() as u16);
    if n < 3 || buf[0] != HANDOFF_MAGIC || buf[1] as usize >= GROUP_SEG.len() {
        return None;
    }
    Some((buf[1] as usize, buf[2]))
}

// misc

pub static mut API_PTR: *const Api = core::ptr::null();

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

// digit input

pub struct DigitInput<const N: usize> {
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

// blobs

#[derive(Clone, Copy)]
pub struct Settings {
    sql_level: u8,
    tail_elimination: bool,
    busy_lock: bool,
    tx_forbid: bool,
    key_auto_lock: u8,
    dual_standby: bool,
    vox_switch: bool,
    vox_level: u8,
    tot_level: u8,
    beeps_switch: bool,
    roger_tone: u8,
    scramble_level: u8,
    rtone: u8,
    scan_mode: u8,
    rit_offset: i8,
    save_level: u8,
    backlight_time: u8,
    channel_display_mode: u8,
    ani_tx: bool,
    rptrl: u8,
    vox_delay: u8,
    battery_cal_raw: u16,
    side1_short: u8,
    side1_long: u8,
    side2_short: u8,
    side2_long: u8,
    band_short: u8,
    band_long: u8,
    band_lock: u8,
    boot_display_mode: u8,
    boot_sound_enabled: bool,
    obs_lat: i32,
    obs_lon: i32,
    boot_text_and_tune: [u8; SETTINGS_BYTES - 40],
}

impl Settings {
    fn from_bytes(buf: &[u8; SETTINGS_BYTES]) -> Settings {
        let mut boot_text_and_tune = [0u8; SETTINGS_BYTES - 40];
        boot_text_and_tune.copy_from_slice(&buf[40..SETTINGS_BYTES]);
        Settings {
            sql_level: buf[0],
            tail_elimination: buf[1] != 0,
            busy_lock: buf[2] != 0,
            tx_forbid: buf[3] != 0,
            key_auto_lock: buf[4],
            dual_standby: buf[5] != 0,
            vox_switch: buf[6] != 0,
            vox_level: buf[7],
            tot_level: buf[8],
            beeps_switch: buf[9] != 0,
            roger_tone: buf[10],
            scramble_level: buf[11],
            rtone: buf[12],
            scan_mode: buf[13],
            rit_offset: buf[14] as i8,
            save_level: buf[15],
            backlight_time: buf[16],
            channel_display_mode: buf[17],
            ani_tx: buf[18] != 0,
            rptrl: buf[19],
            vox_delay: buf[20],
            battery_cal_raw: u16::from_le_bytes([buf[21], buf[22]]),
            side1_short: buf[23],
            side1_long: buf[24],
            side2_short: buf[25],
            side2_long: buf[26],
            band_short: buf[27],
            band_long: buf[28],
            band_lock: buf[29],
            boot_display_mode: buf[30],
            boot_sound_enabled: buf[31] != 0,
            obs_lat: i32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            obs_lon: i32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
            boot_text_and_tune,
        }
    }

    fn to_bytes(self) -> [u8; SETTINGS_BYTES] {
        let mut buf = [0u8; SETTINGS_BYTES];
        buf[0] = self.sql_level;
        buf[1] = self.tail_elimination as u8;
        buf[2] = self.busy_lock as u8;
        buf[3] = self.tx_forbid as u8;
        buf[4] = self.key_auto_lock;
        buf[5] = self.dual_standby as u8;
        buf[6] = self.vox_switch as u8;
        buf[7] = self.vox_level;
        buf[8] = self.tot_level;
        buf[9] = self.beeps_switch as u8;
        buf[10] = self.roger_tone;
        buf[11] = self.scramble_level;
        buf[12] = self.rtone;
        buf[13] = self.scan_mode;
        buf[14] = self.rit_offset as u8;
        buf[15] = self.save_level;
        buf[16] = self.backlight_time;
        buf[17] = self.channel_display_mode;
        buf[18] = self.ani_tx as u8;
        buf[19] = self.rptrl;
        buf[20] = self.vox_delay;
        buf[21..23].copy_from_slice(&self.battery_cal_raw.to_le_bytes());
        buf[23] = self.side1_short;
        buf[24] = self.side1_long;
        buf[25] = self.side2_short;
        buf[26] = self.side2_long;
        buf[27] = self.band_short;
        buf[28] = self.band_long;
        buf[29] = self.band_lock;
        buf[30] = self.boot_display_mode;
        buf[31] = self.boot_sound_enabled as u8;
        buf[32..36].copy_from_slice(&self.obs_lat.to_le_bytes());
        buf[36..40].copy_from_slice(&self.obs_lon.to_le_bytes());
        buf[40..SETTINGS_BYTES].copy_from_slice(&self.boot_text_and_tune);
        buf
    }
}

fn read_settings(api: &Api) -> Settings {
    let mut buf = [0u8; SETTINGS_BYTES];
    (api.settings_read)(buf.as_mut_ptr(), SETTINGS_BYTES as u16);
    Settings::from_bytes(&buf)
}

fn preview_settings(api: &Api, s: &Settings) {
    let buf = s.to_bytes();
    let _ = (api.settings_preview)(buf.as_ptr(), SETTINGS_BYTES as u16);
}

fn commit_settings(api: &Api, s: &Settings) {
    let buf = s.to_bytes();
    let _ = (api.settings_commit)(buf.as_ptr(), SETTINGS_BYTES as u16);
}

#[derive(Clone, Copy)]
pub struct SideCfg {
    wide_band: bool,
    power_raw: u8,
    subaudio_rx_code: u16,
    subaudio_tx_code: u16,
    freq_dir: u8,
    offset_hz: u32,
    ani_target: i16,
    freq_step: u8,
}

impl SideCfg {
    fn from_bytes(buf: &[u8; SIDE_CFG_BYTES]) -> SideCfg {
        SideCfg {
            wide_band: buf[0] != 0,
            power_raw: buf[1],
            subaudio_rx_code: u16::from_le_bytes([buf[2], buf[3]]),
            subaudio_tx_code: u16::from_le_bytes([buf[4], buf[5]]),
            freq_dir: buf[6],
            offset_hz: u32::from_le_bytes([buf[7], buf[8], buf[9], buf[10]]),
            ani_target: i16::from_le_bytes([buf[11], buf[12]]),
            freq_step: buf[13],
        }
    }

    fn to_bytes(self) -> [u8; SIDE_CFG_BYTES] {
        let mut buf = [0u8; SIDE_CFG_BYTES];
        buf[0] = self.wide_band as u8;
        buf[1] = self.power_raw;
        buf[2..4].copy_from_slice(&self.subaudio_rx_code.to_le_bytes());
        buf[4..6].copy_from_slice(&self.subaudio_tx_code.to_le_bytes());
        buf[6] = self.freq_dir;
        buf[7..11].copy_from_slice(&self.offset_hz.to_le_bytes());
        buf[11..13].copy_from_slice(&self.ani_target.to_le_bytes());
        buf[13] = self.freq_step;
        buf
    }
}

fn read_side_cfg(api: &Api) -> SideCfg {
    let mut buf = [0u8; SIDE_CFG_BYTES];
    (api.side_cfg_read)(buf.as_mut_ptr(), SIDE_CFG_BYTES as u16);
    SideCfg::from_bytes(&buf)
}

fn write_side_cfg(api: &Api, s: &SideCfg) {
    let buf = s.to_bytes();
    let _ = (api.side_cfg_write)(buf.as_ptr(), SIDE_CFG_BYTES as u16);
}

// items

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingItem {
    Sql,
    Step,
    Tot,
    Tdr,
    Save,
    Abr,
    BusyLock,
    TxForbid,
    Wn,
    TxPr,
    RxCts,
    TxCts,
    Sftd,
    Offse,
    Beep,
    AutoLk,
    Vox,
    VoxLv,
    VoxDly,
    Scrm,
    Rtone,
    Tail,
    Rptrl,
    Roge,
    ScanMd,
    Rit,
    ChDisp,
    AniTx,
    AniCall,
    Side1Short,
    Side1Long,
    Side2Short,
    Side2Long,
    BandShort,
    BandLong,
    VmLong,
    BootMode,
    BootSnd,
    BattCal,
    FLock,
    Lat,
    Lon,
    Info,
    ChipId,
    Reset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    Radio,
    Sig,
    Display,
    Keys,
    Ani,
    System,
}

pub const SETTINGS_GROUPS: [SettingsGroup; 6] = [
    SettingsGroup::Radio,
    SettingsGroup::Sig,
    SettingsGroup::Display,
    SettingsGroup::Keys,
    SettingsGroup::Ani,
    SettingsGroup::System,
];

impl SettingsGroup {
    fn label(self) -> &'static str {
        match self {
            SettingsGroup::Radio => "RADIO",
            SettingsGroup::Sig => "AUDIO",
            SettingsGroup::Display => "DISPLAY",
            SettingsGroup::Keys => "KEYS",
            SettingsGroup::Ani => "ANI / DTMF",
            SettingsGroup::System => "SYSTEM",
        }
    }

    fn items(self) -> &'static [SettingItem] {
        use SettingItem::*;
        match self {
            #[cfg(feature = "g_radio")]
            SettingsGroup::Radio => &[
                Sql, Step, Tot, Tdr, Save, BusyLock, TxForbid, Wn, TxPr, RxCts,
                TxCts, Sftd, Offse, Scrm, ScanMd, Rit, ChDisp,
            ],
            #[cfg(feature = "g_audio")]
            SettingsGroup::Sig => {
                &[Beep, Vox, VoxLv, VoxDly, Rtone, Tail, Rptrl, Roge]
            }
            #[cfg(feature = "g_disp")]
            SettingsGroup::Display => &[Abr, AutoLk],
            #[cfg(feature = "g_keys")]
            SettingsGroup::Keys => &[
                // BAND's short press always opens the channel groups (core `keys.rs`), so it is not offered
                Side1Short, Side1Long, Side2Short, Side2Long, BandLong, VmLong,
            ],
            #[cfg(feature = "g_ani")]
            SettingsGroup::Ani => &[AniTx, AniCall],
            #[cfg(feature = "g_system")]
            SettingsGroup::System => &[
                FLock, Lat, Lon, BattCal, BootMode, BootSnd, Info, ChipId,
                Reset,
            ],
            #[allow(unreachable_patterns)]
            _ => &[],
        }
    }
}

impl SettingItem {
    fn label(self) -> &'static str {
        use SettingItem::*;
        match self {
            #[cfg(feature = "g_radio")]
            Sql => "SQUELCH",
            #[cfg(feature = "g_radio")]
            Step => "FREQ STEP",
            #[cfg(feature = "g_radio")]
            Tot => "TX TIMEOUT",
            #[cfg(feature = "g_radio")]
            Tdr => "DUAL WATCH",
            #[cfg(feature = "g_radio")]
            Save => "BATTERY SAVE",
            #[cfg(feature = "g_radio")]
            BusyLock => "BUSY LOCKOUT",
            #[cfg(feature = "g_radio")]
            TxForbid => "TX INHIBIT",
            #[cfg(feature = "g_radio")]
            Wn => "BANDWIDTH",
            #[cfg(feature = "g_radio")]
            TxPr => "TX POWER",
            #[cfg(feature = "g_radio")]
            RxCts => "RX TONE",
            #[cfg(feature = "g_radio")]
            TxCts => "TX TONE",
            #[cfg(feature = "g_radio")]
            Sftd => "RPT SHIFT",
            #[cfg(feature = "g_radio")]
            Offse => "RPT OFFSET",
            #[cfg(feature = "g_radio")]
            Scrm => "SCRAMBLER",
            #[cfg(feature = "g_radio")]
            ScanMd => "SCAN RESUME",
            #[cfg(feature = "g_radio")]
            Rit => "RIT",
            #[cfg(feature = "g_radio")]
            ChDisp => "CH DISPLAY",

            #[cfg(feature = "g_audio")]
            Beep => "KEY BEEP",
            #[cfg(feature = "g_audio")]
            Vox => "VOX",
            #[cfg(feature = "g_audio")]
            VoxLv => "VOX LEVEL",
            #[cfg(feature = "g_audio")]
            VoxDly => "VOX DELAY",
            #[cfg(feature = "g_audio")]
            Rtone => "TONE BURST",
            #[cfg(feature = "g_audio")]
            Tail => "SQ TAIL ELIM",
            #[cfg(feature = "g_audio")]
            Rptrl => "RPT TAIL DLY",
            #[cfg(feature = "g_audio")]
            Roge => "ROGER BEEP",

            #[cfg(feature = "g_disp")]
            Abr => "BACKLIGHT",
            #[cfg(feature = "g_disp")]
            AutoLk => "AUTO KEYLOCK",

            #[cfg(feature = "g_keys")]
            Side1Short => "SIDE1 SHORT",
            #[cfg(feature = "g_keys")]
            Side1Long => "SIDE1 LONG",
            #[cfg(feature = "g_keys")]
            Side2Short => "SIDE2 SHORT",
            #[cfg(feature = "g_keys")]
            Side2Long => "SIDE2 LONG",
            #[cfg(feature = "g_keys")]
            BandShort => "BAND SHORT",
            #[cfg(feature = "g_keys")]
            BandLong => "BAND LONG",
            #[cfg(feature = "g_keys")]
            VmLong => "VFO/MR LONG",

            #[cfg(feature = "g_ani")]
            AniTx => "ANI ON PTT",
            #[cfg(feature = "g_ani")]
            AniCall => "ANI CALL",

            #[cfg(feature = "g_system")]
            FLock => "TX BANDS",
            #[cfg(feature = "g_system")]
            Lat => "LATITUDE",
            #[cfg(feature = "g_system")]
            Lon => "LONGITUDE",
            #[cfg(feature = "g_system")]
            BattCal => "BATTERY CAL",
            #[cfg(feature = "g_system")]
            BootMode => "BOOT SCREEN",
            #[cfg(feature = "g_system")]
            BootSnd => "BOOT SOUND",
            #[cfg(feature = "g_system")]
            Info => "VERSION",
            #[cfg(feature = "g_system")]
            ChipId => "RF CHIP ID",
            #[cfg(feature = "g_system")]
            Reset => "RESET ALL",

            #[allow(unreachable_patterns)]
            _ => "",
        }
    }

    fn is_scalar(self) -> bool {
        !matches!(
            self,
            SettingItem::Offse
                | SettingItem::BattCal
                | SettingItem::Lat
                | SettingItem::Lon
                | SettingItem::Info
                | SettingItem::ChipId
                | SettingItem::Reset
        )
    }

    /// RAM-only, lives in `side_cfg` (not the persisted `Settings` blob).
    fn is_side_cfg(self) -> bool {
        matches!(
            self,
            SettingItem::Step
                | SettingItem::Wn
                | SettingItem::TxPr
                | SettingItem::RxCts
                | SettingItem::TxCts
                | SettingItem::Sftd
                | SettingItem::Offse
                | SettingItem::AniCall
        )
    }
}

#[cfg(feature = "g_radio")]
fn power_label(raw: u8) -> &'static str {
    match raw {
        2 => "MID",
        1 => "LOW",
        _ => "HIGH",
    }
}

/// Largest key-function value the core accepts (`keyfn::KEYFN_MAX`).
#[cfg(feature = "g_keys")]
const KEYFN_MAX: i32 = 19;
/// The VFO/MR long-press function: record byte 66 (inside the second boot text line, which the core also uses for
/// the channel group), valid only when the next byte is the marker. See the core's `modes::vm_long`.
#[cfg(feature = "g_keys")]
const VM_LONG_AT: usize = 66 - 40;
#[cfg(feature = "g_keys")]
const VM_LONG_MAGIC: u8 = b'K';

#[cfg(feature = "g_keys")]
fn key_function_label(v: u8) -> &'static str {
    match v {
        1 => "BANDWIDTH",
        2 => "MONITOR",
        3 => "AM/FM",
        4 => "TX TONE",
        6 => "SCAN",
        7 => "TX POWER",
        8 => "LIGHT",
        10 => "REVERSE",
        // 11..=19 open the app in launcher slot 1..=9 (the core's `keyfn::KeyFunction::App`); the APPS list shows the
        // numbers. Values here stay within 9 characters: the longest label plus "^ VALUE v" must fit a 26-character row.
        11 => "APP 1",
        12 => "APP 2",
        13 => "APP 3",
        14 => "APP 4",
        15 => "APP 5",
        16 => "APP 6",
        17 => "APP 7",
        18 => "APP 8",
        19 => "APP 9",
        _ => "NONE",
    }
}

#[cfg(feature = "g_system")]
fn band_lock_label(v: u8) -> &'static str {
    match v {
        1 => "FCC HAM",
        2 => "GB HAM",
        3 => "400-430",
        4 => "400-438",
        5 => "PMR446",
        6 => "GMRS/FRS",
        7 => "CA HAM",
        8 => "NONE",
        9 => "ALL",
        _ => "CE HAM",
    }
}

// help: one plain-language line (26 characters at most) under the list for the selected setting

/// What a key function does (the value of SIDE1 SHORT and the others). 11..=19 are the apps, see `help_into`.
#[cfg(feature = "g_keys")]
fn key_function_help(v: i32) -> &'static str {
    match v {
        1 => "Switches WIDE / NARROW",
        2 => "Squelch open on or off",
        3 => "Steps FM, AM, USB",
        4 => "Transmits the TONE BURST",
        6 => "Starts scanning",
        7 => "Steps HIGH, LOW, MID",
        8 => "Flashlight on or off",
        10 => "Swaps RX and TX frequency",
        _ => "Does nothing",
    }
}

/// The help line for `item` whose current value is `v` (only a few lines depend on it: what a key does, what a
/// battery-save step costs, what a TX BANDS choice allows). Writes at most 26 bytes and returns the length.
fn help_into(item: SettingItem, v: i32, out: &mut [u8]) -> usize {
    use SettingItem::*;
    let s: &str = match item {
        #[cfg(feature = "g_radio")]
        Sql => "Higher mutes weak signals",
        #[cfg(feature = "g_radio")]
        Step => "Step when tuning the VFO",
        #[cfg(feature = "g_radio")]
        Tot => "Stops TX after this time",
        #[cfg(feature = "g_radio")]
        Tdr => "Listen to both A and B",
        #[cfg(feature = "g_radio")]
        Save => match v {
            1 => "Listens 0.1s, sleeps 0.1s",
            2 => "Listens 0.1s, sleeps 0.2s",
            3 => "Listens 0.1s, sleeps 0.3s",
            4 => "Listens 0.1s, sleeps 0.4s",
            _ => "Receiver always listening",
        },
        #[cfg(feature = "g_radio")]
        BusyLock => "No TX while channel busy",
        #[cfg(feature = "g_radio")]
        TxForbid => {
            if v != 0 {
                "Radio will not transmit"
            } else {
                "TX allowed within TX BANDS"
            }
        }
        #[cfg(feature = "g_radio")]
        Wn => "WIDE 25k or NARROW 12.5k",
        #[cfg(feature = "g_radio")]
        TxPr => "Lower power saves battery",
        #[cfg(feature = "g_radio")]
        RxCts => "Hear only calls with tone",
        #[cfg(feature = "g_radio")]
        TxCts => "Tone sent with your TX",
        #[cfg(feature = "g_radio")]
        Sftd => "Repeater TX is + or - RX",
        #[cfg(feature = "g_radio")]
        Offse => "Repeater TX gap, in MHz",
        #[cfg(feature = "g_radio")]
        Scrm => "Voice scramble, must match",
        #[cfg(feature = "g_radio")]
        ScanMd => match v {
            0 => "Waits a bit, then moves on",
            2 => "Stops at the first signal",
            _ => "Holds until signal ends",
        },
        #[cfg(feature = "g_radio")]
        Rit => "Fine RX tune, USB only",
        #[cfg(feature = "g_radio")]
        ChDisp => "Channel shows freq or name",

        #[cfg(feature = "g_audio")]
        Beep => "Beep on keys and alerts",
        #[cfg(feature = "g_audio")]
        Vox => "Talk to transmit, no PTT",
        #[cfg(feature = "g_audio")]
        VoxLv => "1 most sensitive, 9 least",
        #[cfg(feature = "g_audio")]
        VoxDly => "Keeps TX on after speech",
        #[cfg(feature = "g_audio")]
        Rtone => "Pitch of the TX TONE key",
        #[cfg(feature = "g_audio")]
        Tail => "No squelch crash at TX end",
        #[cfg(feature = "g_audio")]
        Rptrl => "Wait after TX before RX",
        #[cfg(feature = "g_audio")]
        Roge => match v {
            1 => "Beep sent when you unkey",
            2 => "Data burst sent at TX end",
            _ => "Nothing sent at TX end",
        },

        #[cfg(feature = "g_disp")]
        Abr => "Light-on time after a key",
        #[cfg(feature = "g_disp")]
        AutoLk => "Locks keys after idle time",

        #[cfg(feature = "g_keys")]
        Side1Short | Side1Long | Side2Short | Side2Long | BandShort | BandLong | VmLong => {
            if (11..=19).contains(&v) {
                let mut w = Out::new(out);
                w.str("Opens the app in slot ");
                w.u32(v as u32 - 10, 1);
                return w.len;
            }
            key_function_help(v)
        }

        #[cfg(feature = "g_ani")]
        AniTx => "Sends your ID when you key",
        #[cfg(feature = "g_ani")]
        AniCall => "Contact your ID goes to",

        #[cfg(feature = "g_system")]
        FLock => match v {
            1 => "2m 1.25m 70cm, FRS, MURS",
            2 => "144-148 430-440 MHz",
            3 => "137-174 400-430 MHz",
            4 => "137-174 400-438 MHz",
            5 => "446.0-446.2 MHz only",
            6 => "FRS, GMRS, MURS only",
            7 => "144-148 430-450 MHz",
            8 => "No transmitting at all",
            9 => "136-174 220-260 400-520",
            _ => "144-146 430-440 MHz",
        },
        #[cfg(feature = "g_system")]
        Lat | Lon => "For HAM TOOLS grid locator",
        #[cfg(feature = "g_system")]
        BattCal => "Type true battery volts",
        #[cfg(feature = "g_system")]
        BootMode => "Logo shown at power-on",
        #[cfg(feature = "g_system")]
        BootSnd => "Start-up tune; needs LOGO",
        #[cfg(feature = "g_system")]
        Info => "MENU: about this firmware",
        #[cfg(feature = "g_system")]
        ChipId => "Radio chip ID, for support",
        #[cfg(feature = "g_system")]
        Reset => "Erases settings and VFOs",

        #[allow(unreachable_patterns)]
        _ => "",
    };
    write_str(out, s)
}

// state

pub struct ItemsState {
    group: SettingsGroup,
    index: usize,
    editing: bool,
    snapshot: i32,
    info_page: u8,
    settings: Settings,
    side_cfg: SideCfg,
    offset_input: DigitInput<7>,
    battery_input: DigitInput<3>,
    lat_input: DigitInput<6>,
    lon_input: DigitInput<7>,
    lat_neg: bool,
    lon_neg: bool,
}

impl ItemsState {
    fn new(api: &Api, group: SettingsGroup) -> ItemsState {
        ItemsState {
            group,
            index: 0,
            editing: false,
            snapshot: 0,
            info_page: 0,
            settings: read_settings(api),
            side_cfg: read_side_cfg(api),
            offset_input: DigitInput::new(),
            battery_input: DigitInput::new(),
            lat_input: DigitInput::new(),
            lon_input: DigitInput::new(),
            lat_neg: false,
            lon_neg: false,
        }
    }

    fn item(&self) -> SettingItem {
        self.group.items()[self.index]
    }

    fn is_editing(&self, index: usize) -> bool {
        self.editing && self.index == index
    }

    fn group_pos(&self) -> usize {
        let mut i = 0;
        while i < SETTINGS_GROUPS.len() {
            if SETTINGS_GROUPS[i] == self.group {
                return i;
            }
            i += 1;
        }
        0
    }
}

enum Phase {
    Groups {
        index: usize,
    },
    Items(ItemsState),
    /// hand off to segment `.0`; `key` turns this into `AppResult::Chain`
    Chain(u8),
    Exit,
}

struct State {
    phase: Phase,
}

static mut STATE: State = State {
    phase: Phase::Groups { index: 0 },
};

// entry

#[cfg_attr(target_os = "none", no_mangle)]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    unsafe {
        API_PTR = api as *const Api;
    }
    match ev {
        AppEvent::Enter => enter(api),
        AppEvent::Key { id, kind } => key(api, kind, id),
        AppEvent::Tick { .. } => AppResult::Continue,
        AppEvent::Draw => draw(api),
        AppEvent::Leave => AppResult::Continue,
    }
}

fn enter(api: &Api) -> AppResult {
    let phase = match handoff_take(api) {
        None => Phase::Groups { index: 0 },
        Some((gi, ACT_GROUPS)) => Phase::Groups { index: gi },
        Some((gi, _)) => {
            let group = SETTINGS_GROUPS[gi];
            if group.items().is_empty() {
                handoff_put(api, gi, ACT_GROUPS);
                return AppResult::Chain(SEG_MENU);
            }
            Phase::Items(ItemsState::new(api, group))
        }
    };
    unsafe {
        STATE.phase = phase;
    }
    AppResult::Continue
}

fn key(api: &Api, kind: u8, id: u8) -> AppResult {
    unsafe {
        let state = &mut *core::ptr::addr_of_mut!(STATE);
        let phase =
            core::mem::replace(&mut state.phase, Phase::Groups { index: 0 });
        state.phase = match phase {
            Phase::Groups { index } => dispatch_groups(api, kind, id, index),
            Phase::Items(st) => dispatch_items(api, kind, id, st),
            other => other,
        };
        match state.phase {
            Phase::Exit => return AppResult::Exit,
            Phase::Chain(seg) => return AppResult::Chain(seg),
            _ => {}
        }
    }
    AppResult::Continue
}

fn open_group(api: &Api, gi: usize) -> Phase {
    let seg = GROUP_SEG[gi];
    if seg == MY_SEG {
        Phase::Items(ItemsState::new(api, SETTINGS_GROUPS[gi]))
    } else {
        handoff_put(api, gi, ACT_ITEMS);
        Phase::Chain(seg)
    }
}

fn back_to_groups(api: &Api, gi: usize) -> Phase {
    if MY_SEG == SEG_MENU {
        Phase::Groups { index: gi }
    } else {
        handoff_put(api, gi, ACT_GROUPS);
        Phase::Chain(SEG_MENU)
    }
}

fn dispatch_groups(api: &Api, kind: u8, id: u8, index: usize) -> Phase {
    if kind != KIND_SINGLE && kind != KIND_REPEAT {
        return Phase::Groups { index };
    }
    let len = SETTINGS_GROUPS.len();
    match id {
        KEY_UP => Phase::Groups {
            index: (index + len - 1) % len,
        },
        KEY_DOWN => Phase::Groups {
            index: (index + 1) % len,
        },
        KEY_MENU if kind == KIND_SINGLE => open_group(api, index),
        KEY_EXIT if kind == KIND_SINGLE => Phase::Exit,
        _ => Phase::Groups { index },
    }
}

fn dispatch_items(api: &Api, kind: u8, id: u8, mut st: ItemsState) -> Phase {
    let item = st.item();

    #[cfg(feature = "g_radio")]
    if st.editing && item == SettingItem::Offse {
        dispatch_offset_input(api, kind, id, &mut st);
        return Phase::Items(st);
    }
    #[cfg(feature = "g_system")]
    if st.editing && item == SettingItem::BattCal {
        dispatch_battery_input(api, kind, id, &mut st);
        return Phase::Items(st);
    }
    #[cfg(feature = "g_system")]
    if st.editing && matches!(item, SettingItem::Lat | SettingItem::Lon) {
        dispatch_coord_input(api, kind, id, item == SettingItem::Lat, &mut st);
        return Phase::Items(st);
    }

    if let Some(phase) = dispatch_items_scalar(api, kind, id, item, &mut st) {
        return phase;
    }
    Phase::Items(st)
}

fn dispatch_items_scalar(
    api: &Api,
    kind: u8,
    id: u8,
    item: SettingItem,
    st: &mut ItemsState,
) -> Option<Phase> {
    // RESET ALL erases settings, VFO and FM data: pressing MENU asks, and only holding # (a different key, held for
    // a second) answers. A second MENU press does nothing, so a double press cannot reset the radio.
    if kind == KIND_LONG && id == KEY_POUND && st.editing && item == SettingItem::Reset {
        (api.factory_reset)() // never returns
    }
    if kind != KIND_SINGLE && kind != KIND_REPEAT {
        return None;
    }
    match id {
        KEY_UP | KEY_DOWN => {
            let up = id == KEY_UP;
            if !st.editing {
                let len = st.group.items().len();
                st.index = if up {
                    (st.index + len - 1) % len
                } else {
                    (st.index + 1) % len
                };
            } else if item == SettingItem::Info {
                st.info_page ^= 1;
            } else if item != SettingItem::Reset {
                adjust(api, st, item, up);
            }
        }
        KEY_DIGIT0 if kind == KIND_SINGLE && st.editing && item.is_scalar() => {
            apply(api, st, item, scalar_floor(item));
        }
        KEY_MENU if kind == KIND_SINGLE => {
            if !st.editing {
                match item {
                    #[cfg(feature = "g_radio")]
                    SettingItem::Offse => st.offset_input.clear(),
                    #[cfg(feature = "g_system")]
                    SettingItem::BattCal => st.battery_input.clear(),
                    #[cfg(feature = "g_system")]
                    SettingItem::Lat => {
                        st.lat_input.clear();
                        let v = st.settings.obs_lat;
                        st.lat_neg = v != COORD_NOT_SET && v < 0;
                    }
                    #[cfg(feature = "g_system")]
                    SettingItem::Lon => {
                        st.lon_input.clear();
                        let v = st.settings.obs_lon;
                        st.lon_neg = v != COORD_NOT_SET && v < 0;
                    }
                    _ => st.snapshot = current_value(api, st, item),
                }
                st.editing = true;
            } else if item == SettingItem::Reset {
                // asked already: see above
            } else {
                st.editing = false;
                if item != SettingItem::Info
                    && item != SettingItem::ChipId
                    && !item.is_side_cfg()
                {
                    commit_settings(api, &st.settings);
                }
            }
        }
        KEY_EXIT if kind == KIND_SINGLE => {
            if st.editing {
                if item != SettingItem::Info
                    && item != SettingItem::ChipId
                    && item != SettingItem::Reset
                {
                    apply(api, st, item, st.snapshot);
                }
                st.editing = false;
            } else {
                return Some(back_to_groups(api, st.group_pos()));
            }
        }
        _ => {}
    }
    None
}

#[cfg(feature = "g_radio")]
fn dispatch_offset_input(api: &Api, kind: u8, id: u8, st: &mut ItemsState) {
    if kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(id) {
        st.offset_input.push(digit);
        if st.offset_input.is_full() {
            commit_offset_input(api, st);
        }
        return;
    }
    match id {
        KEY_MENU => commit_offset_input(api, st),
        KEY_EXIT if st.offset_input.is_empty() => {
            st.editing = false;
        }
        KEY_EXIT => st.offset_input.backspace(),
        _ => {}
    }
}

#[cfg(feature = "g_radio")]
fn commit_offset_input(api: &Api, st: &mut ItemsState) {
    if !st.offset_input.is_empty() {
        let hz = st.offset_input.value() * 100;
        apply(api, st, SettingItem::Offse, hz as i32);
    }
    st.offset_input.clear();
    st.editing = false;
}

#[cfg(feature = "g_system")]
fn dispatch_battery_input(api: &Api, kind: u8, id: u8, st: &mut ItemsState) {
    if kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(id) {
        st.battery_input.push(digit);
        if st.battery_input.is_full() {
            commit_battery_input(api, st);
        }
        return;
    }
    match id {
        KEY_MENU => commit_battery_input(api, st),
        KEY_EXIT if st.battery_input.is_empty() => st.editing = false,
        KEY_EXIT => st.battery_input.backspace(),
        _ => {}
    }
}

#[cfg(feature = "g_system")]
fn commit_battery_input(api: &Api, st: &mut ItemsState) {
    let entered_cv = st.battery_input.value();
    let raw12 = (api.battery_raw12_avg)() as u32;
    if entered_cv > 0 {
        if let Some(new_cal) =
            (raw12 * BATTERY_CAL_REFERENCE_CV).checked_div(entered_cv)
        {
            apply(
                api,
                st,
                SettingItem::BattCal,
                new_cal.clamp(1, u16::MAX as u32) as i32,
            );
            commit_settings(api, &st.settings);
        }
    }
    st.battery_input.clear();
    st.editing = false;
}

#[cfg(feature = "g_system")]
fn dispatch_coord_input(
    api: &Api,
    kind: u8,
    id: u8,
    is_lat: bool,
    st: &mut ItemsState,
) {
    if kind != KIND_SINGLE {
        return;
    }
    if let Some(digit) = digit_value(id) {
        if is_lat {
            st.lat_input.push(digit);
        } else {
            st.lon_input.push(digit);
        }
        return;
    }
    let empty = if is_lat {
        st.lat_input.is_empty()
    } else {
        st.lon_input.is_empty()
    };
    match id {
        KEY_ASTERISK => {
            if is_lat {
                st.lat_neg = !st.lat_neg;
            } else {
                st.lon_neg = !st.lon_neg;
            }
        }
        KEY_MENU => commit_coord_input(api, is_lat, st),
        KEY_EXIT if empty => st.editing = false,
        KEY_EXIT => {
            if is_lat {
                st.lat_input.backspace();
            } else {
                st.lon_input.backspace();
            }
        }
        _ => {}
    }
}

#[cfg(feature = "g_system")]
fn commit_coord_input(api: &Api, is_lat: bool, st: &mut ItemsState) {
    let (typed, degrees, limit, neg, item) = if is_lat {
        (
            !st.lat_input.is_empty(),
            st.lat_input.value(),
            90_00000,
            st.lat_neg,
            SettingItem::Lat,
        )
    } else {
        (
            !st.lon_input.is_empty(),
            st.lon_input.value(),
            180_00000,
            st.lon_neg,
            SettingItem::Lon,
        )
    };
    if typed {
        let mag = (degrees as i32).saturating_mul(10).min(limit);
        apply(api, st, item, if neg { -mag } else { mag });
        commit_settings(api, &st.settings);
    }
    if is_lat {
        st.lat_input.clear();
    } else {
        st.lon_input.clear();
    }
    st.editing = false;
}

// value logic

fn current_value(api: &Api, st: &ItemsState, item: SettingItem) -> i32 {
    use SettingItem::*;
    let _ = api;
    match item {
        #[cfg(feature = "g_radio")]
        Sql => st.settings.sql_level as i32,
        #[cfg(feature = "g_radio")]
        Step => st.side_cfg.freq_step as i32,
        #[cfg(feature = "g_radio")]
        Tot => st.settings.tot_level as i32,
        #[cfg(feature = "g_radio")]
        Tdr => st.settings.dual_standby as i32,
        #[cfg(feature = "g_radio")]
        BusyLock => st.settings.busy_lock as i32,
        #[cfg(feature = "g_radio")]
        TxForbid => st.settings.tx_forbid as i32,
        #[cfg(feature = "g_radio")]
        Wn => (!st.side_cfg.wide_band) as i32,
        #[cfg(feature = "g_radio")]
        TxPr => st.side_cfg.power_raw as i32,
        #[cfg(feature = "g_radio")]
        RxCts => (api.subaudio_index_of_code)(st.side_cfg.subaudio_rx_code),
        #[cfg(feature = "g_radio")]
        TxCts => (api.subaudio_index_of_code)(st.side_cfg.subaudio_tx_code),
        #[cfg(feature = "g_radio")]
        Scrm => st.settings.scramble_level as i32,
        #[cfg(feature = "g_radio")]
        Sftd => st.side_cfg.freq_dir as i32,
        #[cfg(feature = "g_radio")]
        Offse => st.side_cfg.offset_hz as i32,
        #[cfg(feature = "g_radio")]
        ScanMd => st.settings.scan_mode as i32,
        #[cfg(feature = "g_radio")]
        Rit => st.settings.rit_offset as i32,
        #[cfg(feature = "g_radio")]
        Save => st.settings.save_level as i32,
        #[cfg(feature = "g_radio")]
        ChDisp => st.settings.channel_display_mode as i32,

        #[cfg(feature = "g_audio")]
        Beep => st.settings.beeps_switch as i32,
        #[cfg(feature = "g_audio")]
        Roge => st.settings.roger_tone as i32,
        #[cfg(feature = "g_audio")]
        Vox => st.settings.vox_switch as i32,
        #[cfg(feature = "g_audio")]
        VoxLv => st.settings.vox_level as i32,
        #[cfg(feature = "g_audio")]
        VoxDly => st.settings.vox_delay as i32,
        #[cfg(feature = "g_audio")]
        Rtone => st.settings.rtone as i32,
        #[cfg(feature = "g_audio")]
        Tail => st.settings.tail_elimination as i32,
        #[cfg(feature = "g_audio")]
        Rptrl => st.settings.rptrl as i32,

        #[cfg(feature = "g_disp")]
        AutoLk => st.settings.key_auto_lock as i32,
        #[cfg(feature = "g_disp")]
        Abr => st.settings.backlight_time as i32,

        #[cfg(feature = "g_keys")]
        Side1Short => st.settings.side1_short as i32,
        #[cfg(feature = "g_keys")]
        Side1Long => st.settings.side1_long as i32,
        #[cfg(feature = "g_keys")]
        Side2Short => st.settings.side2_short as i32,
        #[cfg(feature = "g_keys")]
        Side2Long => st.settings.side2_long as i32,
        #[cfg(feature = "g_keys")]
        BandShort => st.settings.band_short as i32,
        #[cfg(feature = "g_keys")]
        BandLong => st.settings.band_long as i32,
        #[cfg(feature = "g_keys")]
        VmLong => {
            let t = &st.settings.boot_text_and_tune;
            if t[VM_LONG_AT + 1] == VM_LONG_MAGIC { t[VM_LONG_AT] as i32 } else { 0 }
        }

        #[cfg(feature = "g_ani")]
        AniTx => st.settings.ani_tx as i32,
        #[cfg(feature = "g_ani")]
        AniCall => st.side_cfg.ani_target as i32,

        #[cfg(feature = "g_system")]
        BootMode => st.settings.boot_display_mode as i32,
        #[cfg(feature = "g_system")]
        BootSnd => st.settings.boot_sound_enabled as i32,
        #[cfg(feature = "g_system")]
        BattCal => st.settings.battery_cal_raw as i32,
        #[cfg(feature = "g_system")]
        FLock => st.settings.band_lock as i32,
        #[cfg(feature = "g_system")]
        Lat => st.settings.obs_lat,
        #[cfg(feature = "g_system")]
        Lon => st.settings.obs_lon,
        #[cfg(feature = "g_system")]
        Info => st.info_page as i32,

        #[allow(unreachable_patterns)]
        _ => 0,
    }
}

fn adjust(api: &Api, st: &mut ItemsState, item: SettingItem, up: bool) {
    use SettingItem::*;
    let cur = current_value(api, st, item);
    let _ = st;
    let new_val = match item {
        #[cfg(feature = "g_radio")]
        Sql => clamp_step(cur, up, 0, 9),
        #[cfg(feature = "g_radio")]
        Step => clamp_step(cur, up, 0, STEP_LIST_DECI_HZ.len() as i32 - 1),
        #[cfg(feature = "g_radio")]
        Tot => clamp_step(cur, up, 0, 12),
        #[cfg(feature = "g_radio")]
        Sftd => clamp_step(cur, up, 0, 2),
        #[cfg(feature = "g_radio")]
        Tdr | BusyLock | TxForbid | Wn => 1 - cur,
        #[cfg(feature = "g_radio")]
        TxPr => clamp_step(cur, up, 0, 2),
        #[cfg(feature = "g_radio")]
        RxCts | TxCts => wrap_step(cur, up, -1, SUBAUDIO_MAX_INDEX),
        #[cfg(feature = "g_radio")]
        Scrm => clamp_step(cur, up, 0, 3),
        #[cfg(feature = "g_radio")]
        ScanMd => clamp_step(cur, up, 0, 2),
        #[cfg(feature = "g_radio")]
        Rit => clamp_step(cur, up, -127, 127),
        #[cfg(feature = "g_radio")]
        Save => clamp_step(cur, up, 0, 4),
        #[cfg(feature = "g_radio")]
        ChDisp => clamp_step(cur, up, 0, 2),

        #[cfg(feature = "g_audio")]
        Beep | Tail | Vox => 1 - cur,
        #[cfg(feature = "g_audio")]
        Roge => clamp_step(cur, up, 0, 2),
        #[cfg(feature = "g_audio")]
        VoxDly => clamp_step(cur, up, 0, 15),
        #[cfg(feature = "g_audio")]
        VoxLv => clamp_step(cur, up, 1, 9),
        #[cfg(feature = "g_audio")]
        Rtone => clamp_step(cur, up, 0, 3),
        #[cfg(feature = "g_audio")]
        Rptrl => clamp_step(cur, up, 0, 10),

        #[cfg(feature = "g_disp")]
        AutoLk => clamp_step(cur, up, 0, 3),
        #[cfg(feature = "g_disp")]
        Abr => clamp_step(cur, up, 0, 4),

        #[cfg(feature = "g_keys")]
        Side1Short | Side1Long | Side2Short | Side2Long | BandShort
        | BandLong | VmLong => {
            // 5 and 9 are retired functions (FM and search became apps): never land on them
            let mut v = clamp_step(cur, up, 0, KEYFN_MAX);
            while v == 5 || v == 9 {
                v = clamp_step(v, up, 0, KEYFN_MAX);
            }
            v
        }

        #[cfg(feature = "g_ani")]
        AniTx => 1 - cur,
        #[cfg(feature = "g_ani")]
        AniCall => clamp_step(cur, up, -1, CONTACT_COUNT - 1),

        #[cfg(feature = "g_system")]
        BootMode => if up { 2 } else { 0 }, // 1 (message) draws the same logo: not offered
        #[cfg(feature = "g_system")]
        BootSnd => {
            if st.settings.boot_display_mode == 0 {
                cur
            } else {
                1 - cur
            }
        }
        #[cfg(feature = "g_system")]
        FLock => clamp_step(cur, up, 0, 9),

        #[allow(unreachable_patterns)]
        _ => cur,
    };
    apply(api, st, item, new_val);
}

fn scalar_floor(item: SettingItem) -> i32 {
    match item {
        SettingItem::AniCall | SettingItem::RxCts | SettingItem::TxCts => -1,
        SettingItem::VoxLv => 1,
        _ => 0,
    }
}

fn apply(api: &Api, st: &mut ItemsState, item: SettingItem, v: i32) {
    use SettingItem::*;
    let _ = v;
    match item {
        #[cfg(feature = "g_radio")]
        Sql => st.settings.sql_level = v as u8,
        #[cfg(feature = "g_radio")]
        Step => st.side_cfg.freq_step = v as u8,
        #[cfg(feature = "g_radio")]
        Tot => st.settings.tot_level = v as u8,
        #[cfg(feature = "g_radio")]
        Tdr => st.settings.dual_standby = v != 0,
        #[cfg(feature = "g_radio")]
        BusyLock => st.settings.busy_lock = v != 0,
        #[cfg(feature = "g_radio")]
        TxForbid => st.settings.tx_forbid = v != 0,
        #[cfg(feature = "g_radio")]
        Wn => st.side_cfg.wide_band = v == 0,
        #[cfg(feature = "g_radio")]
        TxPr => st.side_cfg.power_raw = v as u8,
        #[cfg(feature = "g_radio")]
        RxCts => st.side_cfg.subaudio_rx_code = (api.subaudio_code_of_index)(v),
        #[cfg(feature = "g_radio")]
        TxCts => st.side_cfg.subaudio_tx_code = (api.subaudio_code_of_index)(v),
        #[cfg(feature = "g_radio")]
        Scrm => st.settings.scramble_level = v as u8,
        #[cfg(feature = "g_radio")]
        Sftd => st.side_cfg.freq_dir = v as u8,
        #[cfg(feature = "g_radio")]
        Offse => st.side_cfg.offset_hz = v as u32,
        #[cfg(feature = "g_radio")]
        ScanMd => st.settings.scan_mode = v as u8,
        #[cfg(feature = "g_radio")]
        Save => st.settings.save_level = v as u8,
        #[cfg(feature = "g_radio")]
        ChDisp => st.settings.channel_display_mode = v as u8,
        #[cfg(feature = "g_radio")]
        Rit => st.settings.rit_offset = v as i8,

        #[cfg(feature = "g_audio")]
        Beep => st.settings.beeps_switch = v != 0,
        #[cfg(feature = "g_audio")]
        Roge => st.settings.roger_tone = v as u8,
        #[cfg(feature = "g_audio")]
        Vox => st.settings.vox_switch = v != 0,
        #[cfg(feature = "g_audio")]
        VoxLv => st.settings.vox_level = v as u8,
        #[cfg(feature = "g_audio")]
        VoxDly => st.settings.vox_delay = v as u8,
        #[cfg(feature = "g_audio")]
        Rtone => st.settings.rtone = v as u8,
        #[cfg(feature = "g_audio")]
        Tail => st.settings.tail_elimination = v != 0,
        #[cfg(feature = "g_audio")]
        Rptrl => st.settings.rptrl = v as u8,

        #[cfg(feature = "g_disp")]
        AutoLk => st.settings.key_auto_lock = v as u8,
        #[cfg(feature = "g_disp")]
        Abr => st.settings.backlight_time = v as u8,

        #[cfg(feature = "g_keys")]
        Side1Short => st.settings.side1_short = v as u8,
        #[cfg(feature = "g_keys")]
        Side1Long => st.settings.side1_long = v as u8,
        #[cfg(feature = "g_keys")]
        Side2Short => st.settings.side2_short = v as u8,
        #[cfg(feature = "g_keys")]
        Side2Long => st.settings.side2_long = v as u8,
        #[cfg(feature = "g_keys")]
        BandShort => st.settings.band_short = v as u8,
        #[cfg(feature = "g_keys")]
        BandLong => st.settings.band_long = v as u8,
        #[cfg(feature = "g_keys")]
        VmLong => {
            st.settings.boot_text_and_tune[VM_LONG_AT] = v as u8;
            st.settings.boot_text_and_tune[VM_LONG_AT + 1] = VM_LONG_MAGIC;
        }

        #[cfg(feature = "g_ani")]
        AniTx => st.settings.ani_tx = v != 0,
        #[cfg(feature = "g_ani")]
        AniCall => st.side_cfg.ani_target = v as i16,

        #[cfg(feature = "g_system")]
        BootMode => {
            st.settings.boot_display_mode = v as u8;
            if v == 0 {
                st.settings.boot_sound_enabled = false;
            }
        }
        #[cfg(feature = "g_system")]
        BootSnd => st.settings.boot_sound_enabled = v != 0,
        #[cfg(feature = "g_system")]
        BattCal => st.settings.battery_cal_raw = v as u16,
        #[cfg(feature = "g_system")]
        Lat => st.settings.obs_lat = v,
        #[cfg(feature = "g_system")]
        Lon => st.settings.obs_lon = v,
        #[cfg(feature = "g_system")]
        FLock => st.settings.band_lock = v as u8,

        #[allow(unreachable_patterns)]
        _ => {}
    }
    if item.is_side_cfg() {
        write_side_cfg(api, &st.side_cfg);
    } else {
        preview_settings(api, &st.settings);
    }
}

// formatting

fn write_str(out: &mut [u8], s: &str) -> usize {
    let n = s.len().min(out.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < n {
        out[i] = bytes[i];
        i += 1;
    }
    n
}

fn on_off(out: &mut [u8], v: i32) -> usize {
    write_str(out, if v != 0 { "ON" } else { "OFF" })
}

struct Out<'a> {
    data: &'a mut [u8],
    len: usize,
}

impl<'a> Out<'a> {
    fn new(data: &'a mut [u8]) -> Self {
        Out { data, len: 0 }
    }

    fn byte(&mut self, b: u8) {
        if self.len < self.data.len() {
            self.data[self.len] = b;
            self.len += 1;
        }
    }

    fn str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.byte(b);
        }
    }

    /// Decimal, left-padded with `'0'` to at least `pad` digits (`pad <= 1`
    /// means "no padding"; `0` still prints as `0`).
    fn u32(&mut self, v: u32, pad: usize) {
        let mut buf = [0u8; 10];
        let mut n = 0;
        let mut x = v;
        loop {
            buf[n] = b'0' + (x % 10) as u8;
            n += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        while n < pad && n < buf.len() {
            buf[n] = b'0';
            n += 1;
        }
        while n > 0 {
            n -= 1;
            self.byte(buf[n]);
        }
    }

    /// `force_sign` prints `+` for non-negatives.
    fn i32(&mut self, v: i32, force_sign: bool) {
        if v < 0 {
            self.byte(b'-');
        } else if force_sign {
            self.byte(b'+');
        }
        self.u32(v.unsigned_abs(), 1);
    }
}

fn value_text_for(
    api: &Api,
    st: &ItemsState,
    gi: usize,
    out: &mut [u8; 18],
) -> usize {
    use SettingItem::*;
    let item = st.group.items()[gi];
    let editing_this = st.is_editing(gi);

    if editing_this {
        #[cfg(feature = "g_radio")]
        if item == Offse {
            return st.offset_input.write_display_into(3, out);
        }
        #[cfg(feature = "g_system")]
        if item == BattCal {
            return st.battery_input.write_display_into(1, out);
        }
        #[cfg(feature = "g_system")]
        if matches!(item, Lat | Lon) {
            let is_lat = item == Lat;
            let (n, neg) = if is_lat {
                (st.lat_input.write_display_into(2, out), st.lat_neg)
            } else {
                (st.lon_input.write_display_into(3, out), st.lon_neg)
            };
            out[n] = match (is_lat, neg) {
                (true, false) => b'N',
                (true, true) => b'S',
                (false, false) => b'E',
                (false, true) => b'W',
            };
            return n + 1;
        }
    }

    match item {
        #[cfg(feature = "g_system")]
        Info => {
            let s = if st.info_page == 0 {
                "UV-5RM OPEN FW"
            } else {
                "SEE BOOT SCREEN"
            };
            write_str(out, s)
        }
        #[cfg(feature = "g_system")]
        ChipId => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let v = (api.settings_get)(7);
            out[0] = b'0';
            out[1] = b'x';
            for i in 0..4 {
                out[2 + i] = HEX[((v >> ((3 - i) * 4)) & 0xF) as usize];
            }
            6
        }
        #[cfg(feature = "g_system")]
        Reset => {
            if editing_this {
                write_str(out, "HOLD #")
            } else {
                0
            }
        }
        #[cfg(feature = "g_system")]
        BootSnd => on_off(out, current_value(api, st, item)),
        #[cfg(feature = "g_system")]
        BootMode => write_str(
            out,
            match current_value(api, st, item) {
                0 => "OFF",
                _ => "LOGO",
            },
        ),
        #[cfg(feature = "g_system")]
        BattCal => {
            let raw12 = (api.battery_raw12_avg)() as u32;
            let cv = raw12 * BATTERY_CAL_REFERENCE_CV
                / (st.settings.battery_cal_raw as u32).max(1);
            let mut w = Out::new(out);
            w.u32(cv / 100, 1);
            w.byte(b'.');
            w.u32(cv % 100, 2);
            w.str("V");
            w.len
        }
        #[cfg(feature = "g_system")]
        Lat | Lon => {
            let is_lat = item == Lat;
            let v = current_value(api, st, item);
            if v == COORD_NOT_SET {
                return write_str(out, "----");
            }
            let av = v.unsigned_abs();
            let mut w = Out::new(out);
            w.u32(av / 100_000, 1);
            w.byte(b'.');
            w.u32((av % 100_000) / 10, 4);
            w.byte(match (is_lat, v < 0) {
                (true, false) => b'N',
                (true, true) => b'S',
                (false, false) => b'E',
                (false, true) => b'W',
            });
            w.len
        }
        #[cfg(feature = "g_system")]
        FLock => {
            let idx = current_value(api, st, item).clamp(0, 9) as u8;
            write_str(out, band_lock_label(idx))
        }

        #[cfg(feature = "g_radio")]
        Step => {
            let deci_hz =
                STEP_LIST_DECI_HZ[current_value(api, st, item) as usize];
            let mut w = Out::new(out);
            w.u32(deci_hz / 100, 1);
            w.byte(b'.');
            w.u32(deci_hz % 100, 2);
            w.str("k");
            w.len
        }
        #[cfg(feature = "g_radio")]
        Tdr | BusyLock | TxForbid => on_off(out, current_value(api, st, item)),
        #[cfg(feature = "g_radio")]
        Wn => write_str(
            out,
            if current_value(api, st, item) != 0 {
                "NARROW"
            } else {
                "WIDE"
            },
        ),
        #[cfg(feature = "g_radio")]
        TxPr => write_str(out, power_label(current_value(api, st, item) as u8)),
        #[cfg(feature = "g_radio")]
        RxCts | TxCts => {
            let v = current_value(api, st, item);
            (api.subaudio_format)(v, out.as_mut_ptr(), 18) as usize
        }
        #[cfg(feature = "g_radio")]
        Offse => {
            let hz = current_value(api, st, item) as u32;
            let mut w = Out::new(out);
            w.u32(hz / 1_000_000, 1);
            w.byte(b'.');
            w.u32((hz % 1_000_000) / 100, 4);
            w.len
        }
        #[cfg(feature = "g_radio")]
        ScanMd => write_str(
            out,
            match current_value(api, st, item) {
                0 => "TIMED",
                2 => "STOP",
                _ => "CARRIER",
            },
        ),
        #[cfg(feature = "g_radio")]
        Rit => {
            let hz = current_value(api, st, item) * RIT_STEP_HZ;
            let mut w = Out::new(out);
            if hz == 0 {
                w.str("0");
            } else {
                w.i32(hz, true);
            }
            w.str("Hz");
            w.len
        }
        #[cfg(feature = "g_radio")]
        Scrm | Save => {
            let v = current_value(api, st, item);
            if v == 0 {
                write_str(out, "OFF")
            } else {
                let mut w = Out::new(out);
                w.u32(v as u32, 1);
                w.len
            }
        }
        #[cfg(feature = "g_radio")]
        ChDisp => write_str(
            out,
            match current_value(api, st, item) {
                1 => "NAME",
                2 => "NAME+FREQ",
                _ => "FREQUENCY",
            },
        ),

        #[cfg(feature = "g_audio")]
        Beep | Tail | Vox => on_off(out, current_value(api, st, item)),
        #[cfg(feature = "g_audio")]
        VoxDly => {
            let deci_s = 5 + current_value(api, st, item);
            let mut w = Out::new(out);
            w.u32((deci_s / 10) as u32, 1);
            w.byte(b'.');
            w.u32((deci_s % 10) as u32, 1);
            w.str("S");
            w.len
        }
        #[cfg(feature = "g_audio")]
        Rptrl => {
            let v = current_value(api, st, item);
            if v == 0 {
                write_str(out, "OFF")
            } else {
                let mut w = Out::new(out);
                w.u32((v * 100) as u32, 1);
                w.str("MS");
                w.len
            }
        }
        #[cfg(feature = "g_audio")]
        Roge => write_str(
            out,
            match current_value(api, st, item) {
                1 => "ROGER",
                2 => "MDC1200",
                _ => "OFF",
            },
        ),
        #[cfg(feature = "g_audio")]
        Rtone => {
            let idx = current_value(api, st, item).clamp(0, 3) as usize;
            let mut w = Out::new(out);
            w.u32(RTONE_HZ_DIV_10[idx] as u32 * 10, 1);
            w.str("Hz");
            w.len
        }

        #[cfg(feature = "g_disp")]
        AutoLk => write_str(
            out,
            match current_value(api, st, item) {
                1 => "5 SEC",
                2 => "10 SEC",
                3 => "15 SEC",
                _ => "OFF",
            },
        ),
        #[cfg(feature = "g_disp")]
        Abr => write_str(
            out,
            match current_value(api, st, item) {
                1 => "5 SEC",
                2 => "10 SEC",
                3 => "15 SEC",
                4 => "20 SEC",
                _ => "ALWAYS ON",
            },
        ),

        #[cfg(feature = "g_keys")]
        Side1Short | Side1Long | Side2Short | Side2Long | BandShort
        | BandLong | VmLong => {
            let idx = current_value(api, st, item).clamp(0, KEYFN_MAX) as u8;
            write_str(out, key_function_label(idx))
        }

        #[cfg(feature = "g_ani")]
        AniTx => on_off(out, current_value(api, st, item)),

        // 0 = no limit, otherwise steps of 15 seconds (the core's `tot_seconds`)
        #[cfg(feature = "g_radio")]
        Tot => {
            let v = current_value(api, st, item);
            if v == 0 {
                write_str(out, "OFF")
            } else {
                let mut w = Out::new(out);
                w.u32(v as u32 * 15, 1);
                w.str(" SEC");
                w.len
            }
        }
        // the same words CHAN MGR uses for this field
        #[cfg(feature = "g_radio")]
        Sftd => write_str(
            out,
            match current_value(api, st, item) {
                1 => "+",
                2 => "-",
                _ => "OFF",
            },
        ),
        // contact index from 0; shown as CHAN MGR shows it
        #[cfg(feature = "g_ani")]
        AniCall => {
            let v = current_value(api, st, item);
            if v < 0 {
                write_str(out, "NONE")
            } else {
                let mut w = Out::new(out);
                w.byte(b'#');
                w.u32(v as u32 + 1, 1);
                w.len
            }
        }

        #[allow(unreachable_patterns)]
        _ => {
            let mut w = Out::new(out);
            w.i32(current_value(api, st, item), false);
            w.len
        }
    }
}

// draw

fn scroll_top(total: usize, selected: usize) -> usize {
    if total <= VISIBLE_ROWS {
        0
    } else {
        selected
            .saturating_sub(VISIBLE_ROWS / 2)
            .min(total - VISIBLE_ROWS)
    }
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
        Phase::Groups { index } => (SETTINGS_GROUPS.len(), *index, false),
        Phase::Items(st) => (
            st.group.items().len(),
            st.index,
            st.editing
                && !matches!(
                    st.item(),
                    // typed fields, and the reset confirmation (UP/DN do nothing there)
                    SettingItem::Offse
                        | SettingItem::BattCal
                        | SettingItem::Lat
                        | SettingItem::Lon
                        | SettingItem::Reset
                ),
        ),
        _ => return AppResult::Continue,
    };
    let window_start = scroll_top(total, selected);

    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        for i in 0..VISIBLE_ROWS {
            rows[i] = ListRow {
                label: [0; 16],
                value: [0; 18],
                has_value: false,
                cursor: -1,
            };
            let gi = window_start + i;
            if gi >= total {
                continue;
            }
            match &state.phase {
                Phase::Groups { .. } => {
                    write_str(&mut rows[i].label, SETTINGS_GROUPS[gi].label());
                }
                Phase::Items(st) => {
                    let item = st.group.items()[gi];
                    write_str(&mut rows[i].label, item.label());
                    let n = value_text_for(api, st, gi, &mut rows[i].value);
                    rows[i].has_value = n > 0;
                    // a typed field shows a text cursor after what has been typed (and so the "changing" colour)
                    if st.is_editing(gi)
                        && matches!(
                            item,
                            SettingItem::Offse | SettingItem::BattCal | SettingItem::Lat | SettingItem::Lon
                        )
                    {
                        rows[i].cursor = n.min(17) as i16;
                    }
                    if st.is_editing(gi) && matches!(item, SettingItem::Reset) {
                        rows[i].cursor = bf5rh_abi::ROW_ARMED;
                    }
                }
                _ => {}
            }
        }
    }

    let mut title_buf = [0u8; 16];
    match &state.phase {
        Phase::Groups { .. } => {
            write_str(&mut title_buf, "SETTINGS");
        }
        Phase::Items(st) => {
            write_str(&mut title_buf, st.group.label());
        }
        _ => {}
    }

    // The strip at the bottom: while looking at a list it explains the selected setting (the keys there never
    // change: MENU changes it, EXIT goes back); while a value is being changed it says what the keys do
    // (EXIT while changing a value undoes it).
    let mut help_buf = [0u8; 26];
    let hint: &[u8] = match &state.phase {
        Phase::Items(st) if st.editing => match st.item() {
            SettingItem::Reset => b"HOLD # reset  EXIT cancel",
            SettingItem::Info | SettingItem::ChipId => b"UP/DN page    EXIT back",
            SettingItem::Lat => b"0-9  * N/S  MENU ok",
            SettingItem::Lon => b"0-9  * E/W  MENU ok",
            SettingItem::Offse | SettingItem::BattCal => b"0-9 type MENU ok EXIT del",
            _ => b"UP/DN MENU save EXIT undo",
        },
        Phase::Items(st) => {
            let item = st.item();
            let n = help_into(item, current_value(api, st, item), &mut help_buf);
            &help_buf[..n]
        }
        _ => b"MENU open    EXIT back",
    };
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);

    let rows_ptr = core::ptr::addr_of!(ROWS) as *const ListRow;
    (api.draw_list)(
        title_buf.as_ptr(),
        16,
        rows_ptr,
        VISIBLE_ROWS as u16,
        window_start as u16,
        selected as u16,
        total as u16,
        show_arrows,
    );

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

// Host tests. The simulator crate runs them once per segment build (`dev.sh unit`: --features settings-menu, -radio,
// -system), because each segment compiles only its own groups.
#[cfg(test)]
mod tests {
    use super::*;

    /// Every setting a segment shows has a help line, for every value it can have, and the line fits the strip.
    #[test]
    fn every_setting_has_help_that_fits_the_strip() {
        let mut seen = 0;
        for g in SETTINGS_GROUPS {
            for &item in g.items() {
                seen += 1;
                for v in -1..=300 {
                    let mut buf = [0u8; 64];
                    let n = help_into(item, v, &mut buf);
                    assert!(n > 0, "{}: no help for value {v}", item.label());
                    assert!(n <= 26, "{}: help for value {v} is {n} characters", item.label());
                    assert!(buf[..n].iter().all(|b| (0x20..0x7F).contains(b)), "{}: not plain ASCII", item.label());
                }
            }
        }
        assert!(seen > 0 || !have_group(0) && !have_group(1) && !have_group(5));
    }
}
