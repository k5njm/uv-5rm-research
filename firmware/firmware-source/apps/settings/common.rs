// One source, built once per segment with only that segment's groups (cargo features): what a segment does not
// show is unused there.
#![allow(dead_code, unused_imports, unused_variables, unreachable_code)]

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
pub const SEG_PRESETS: u8 = 3;

pub const GROUP_SEG: [u8; 7] = [
    SEG_RADIO,  // Radio
    SEG_MENU,   // Sig
    SEG_MENU,   // Display
    SEG_MENU,   // Keys
    SEG_MENU,   // Ani
    SEG_SYSTEM, // System
    SEG_PRESETS, // Presets
];

/// Does this segment show any group of settings to edit? (Segment PRESETS only lists presets and shows two views, so the
/// code that steps, types and restores values is dead there and the linker drops it.)
const EDITS_SETTINGS: bool = cfg!(any(
    feature = "g_radio",
    feature = "g_audio",
    feature = "g_disp",
    feature = "g_keys",
    feature = "g_ani",
    feature = "g_system"
));

/// `true` when this segment compiled the items of group `gi`.
const fn have_group(gi: usize) -> bool {
    match gi {
        0 => cfg!(feature = "g_radio"),
        1 => cfg!(feature = "g_audio"),
        2 => cfg!(feature = "g_disp"),
        3 => cfg!(feature = "g_keys"),
        4 => cfg!(feature = "g_ani"),
        5 => cfg!(feature = "g_system"),
        _ => cfg!(feature = "g_presets"),
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
/// segment PRESETS shows the about view (byte 1: the row to start on)
const ACT_ABOUT: u8 = 2;
/// segment PRESETS takes a grid locator
const ACT_GRID: u8 = 3;
/// SYSTEM's place in the group list: the views of segment PRESETS come back to it
const SYSTEM_GROUP: usize = 5;

/// The handoff buffer: magic, a group (for the views: the row they start on), an action (`ACT_*`) and the row of the
/// list to come back to (absent in older three-byte buffers: 0).
fn handoff_encode(group: usize, action: u8, row: u8) -> [u8; 4] {
    [HANDOFF_MAGIC, group as u8, action, row]
}

fn handoff_decode(buf: &[u8; 4], n: usize) -> Option<(usize, u8, u8)> {
    if n < 3 || buf[0] != HANDOFF_MAGIC || buf[1] as usize >= GROUP_SEG.len() {
        return None;
    }
    Some((buf[1] as usize, buf[2], if n >= 4 { buf[3] } else { 0 }))
}

fn handoff_put(api: &Api, group: usize, action: u8, row: u8) {
    let buf = handoff_encode(group, action, row);
    let _ = (api.handoff_write)(buf.as_ptr(), buf.len() as u16);
}

fn handoff_take(api: &Api) -> Option<(usize, u8, u8)> {
    let mut buf = [0u8; 4];
    let n = (api.handoff_read)(buf.as_mut_ptr(), buf.len() as u16);
    handoff_decode(&buf, n as usize)
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
    Grid,
    Info,
    ChipId,
    Reset,
    // the rows of the PRESETS group
    PresetQuiet,
    PresetBattery,
    PresetScan,
    PresetRepeater,
    PresetDefaults,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    Radio,
    Sig,
    Display,
    Keys,
    Ani,
    System,
    Presets,
}

pub const SETTINGS_GROUPS: [SettingsGroup; 7] = [
    SettingsGroup::Radio,
    SettingsGroup::Sig,
    SettingsGroup::Display,
    SettingsGroup::Keys,
    SettingsGroup::Ani,
    SettingsGroup::System,
    SettingsGroup::Presets,
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
            SettingsGroup::Presets => "PRESETS",
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
                FLock, Lat, Lon, Grid, BattCal, BootMode, BootSnd, Info, ChipId,
                Reset,
            ],
            #[cfg(feature = "g_presets")]
            SettingsGroup::Presets => &[PresetQuiet, PresetBattery, PresetScan, PresetRepeater, PresetDefaults],
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
            Grid => "GRID LOCATOR",
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

            #[cfg(feature = "g_presets")]
            PresetQuiet => "QUIET",
            #[cfg(feature = "g_presets")]
            PresetBattery => "BATTERY SAVER",
            #[cfg(feature = "g_presets")]
            PresetScan => "SCANNING",
            #[cfg(feature = "g_presets")]
            PresetRepeater => "REPEATER",
            #[cfg(feature = "g_presets")]
            PresetDefaults => "DEFAULTS",

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
                | SettingItem::Grid
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
        Grid => "Sets LATITUDE + LONGITUDE",
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

        #[cfg(feature = "g_presets")]
        PresetQuiet => "No beeps, short light",
        #[cfg(feature = "g_presets")]
        PresetBattery => "Sleeps more, light short",
        #[cfg(feature = "g_presets")]
        PresetScan => "Scanning: stays on a call",
        #[cfg(feature = "g_presets")]
        PresetRepeater => "Polite repeater operating",
        #[cfg(feature = "g_presets")]
        PresetDefaults => "Defaults, except TX limits",

        #[allow(unreachable_patterns)]
        _ => "",
    };
    write_str(out, s)
}

// defaults

/// The value every setting has in a new settings record (what a factory reset produces), in the item's own terms
/// (`current_value`). The first group is the core's `Settings::DEFAULT`; a host test (`defaults_match_the_core`) reads
/// the core's source and fails when this table and that one disagree. The RAM-only items (they belong to the VFO,
/// not to the record) take the VFO's start-up values. None: the item has no value to restore.
fn default_of(item: SettingItem) -> Option<i32> {
    use SettingItem::*;
    Some(match item {
        #[cfg(feature = "g_radio")]
        Sql => 3,
        #[cfg(feature = "g_radio")]
        Tot => 8,
        #[cfg(feature = "g_radio")]
        Tdr | Save | BusyLock | Scrm | Rit | Sftd | Offse | Wn => 0,
        #[cfg(feature = "g_radio")]
        TxForbid => 1,
        #[cfg(feature = "g_radio")]
        ScanMd | TxPr => 1,
        #[cfg(feature = "g_radio")]
        ChDisp => 2,
        #[cfg(feature = "g_radio")]
        Step => 3,
        #[cfg(feature = "g_radio")]
        RxCts | TxCts => -1,

        #[cfg(feature = "g_audio")]
        Beep | Tail | VoxLv => 1,
        #[cfg(feature = "g_audio")]
        Vox | Rptrl | Roge => 0,
        #[cfg(feature = "g_audio")]
        VoxDly => 5,
        #[cfg(feature = "g_audio")]
        Rtone => 2,

        #[cfg(feature = "g_disp")]
        Abr => 2,
        #[cfg(feature = "g_disp")]
        AutoLk => 0,

        #[cfg(feature = "g_keys")]
        Side1Short => 5,
        #[cfg(feature = "g_keys")]
        Side2Short => 8,
        #[cfg(feature = "g_keys")]
        BandShort => 6,
        #[cfg(feature = "g_keys")]
        Side1Long | Side2Long | BandLong | VmLong => 0,

        #[cfg(feature = "g_ani")]
        AniTx => 0,
        #[cfg(feature = "g_ani")]
        AniCall => -1,

        #[cfg(feature = "g_system")]
        FLock => 1,
        #[cfg(feature = "g_system")]
        Lat | Lon => COORD_NOT_SET,
        #[cfg(feature = "g_system")]
        BattCal => 2537,
        #[cfg(feature = "g_system")]
        BootMode => 2,
        #[cfg(feature = "g_system")]
        BootSnd => 1,

        #[allow(unreachable_patterns)]
        _ => return None,
    })
}

/// May `#` put `item` (now at `cur`) back to its default? Never when that would loosen a transmit restriction:
/// TX INHIBIT's default is ON (the strictest), so it always may; TX BANDS' default (FCC HAM) allows more than most
/// choices, so it is restored only from a choice that already allows everything FCC HAM does (FCC HAM itself, or
/// ALL), and otherwise has to be chosen with UP and DOWN like any other change.
fn default_allowed(item: SettingItem, cur: i32) -> bool {
    match item {
        SettingItem::FLock => cur == 1 || cur == 9,
        _ => true,
    }
}

/// `#` while a value is being changed: the item goes back to its default (shown at once, like any change; MENU keeps
/// it, EXIT puts the old value back).
fn back_to_default(api: &Api, st: &mut ItemsState, item: SettingItem) {
    let Some(d) = default_of(item) else {
        return;
    };
    if !default_allowed(item, current_value(api, st, item)) {
        st.note = 1;
        return;
    }
    apply(api, st, item, d);
    if !item.is_scalar() {
        st.defaulted = true;
        st.offset_input.clear();
        st.battery_input.clear();
        st.lat_input.clear();
        st.lon_input.clear();
    }
}

// state

pub struct ItemsState {
    group: SettingsGroup,
    index: usize,
    editing: bool,
    snapshot: i32,
    settings: Settings,
    side_cfg: SideCfg,
    offset_input: DigitInput<7>,
    battery_input: DigitInput<3>,
    lat_input: DigitInput<6>,
    lon_input: DigitInput<7>,
    lat_neg: bool,
    lon_neg: bool,
    /// `#` put the value being changed back to its default (a typed field then behaves like any other row)
    defaulted: bool,
    /// a line for the strip instead of the keys, until the next key: 0 none, 1 default refused
    note: u8,
    /// PRESETS only: 0 = the list of presets, else the preview of preset `view - 1`; `pidx` is the row selected there
    view: u8,
    pidx: usize,
    /// PRESETS only: the "applied" notice is on the screen
    toast: bool,
}

impl ItemsState {
    fn new(api: &Api, group: SettingsGroup, index: usize) -> ItemsState {
        ItemsState {
            group,
            index: index.min(group.items().len().saturating_sub(1)),
            editing: false,
            snapshot: 0,
            settings: read_settings(api),
            side_cfg: read_side_cfg(api),
            offset_input: DigitInput::new(),
            battery_input: DigitInput::new(),
            lat_input: DigitInput::new(),
            lon_input: DigitInput::new(),
            lat_neg: false,
            lon_neg: false,
            defaulted: false,
            note: 0,
            view: 0,
            pidx: 0,
            toast: false,
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
    /// segment PRESETS: the about view (cursor row, and the SYSTEM row to come back to)
    #[cfg(feature = "g_presets")]
    About { cur: u8, back: u8 },
    /// segment PRESETS: entry of a grid locator
    #[cfg(feature = "g_presets")]
    Grid(GridState),
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
        Some((gi, ACT_GROUPS, _)) => Phase::Groups { index: gi },
        #[cfg(feature = "g_presets")]
        Some((cur, ACT_ABOUT, back)) => Phase::About { cur: cur as u8, back },
        #[cfg(feature = "g_presets")]
        Some((_, ACT_GRID, back)) => Phase::Grid(GridState::new(back)),
        Some((gi, _, row)) => {
            let group = SETTINGS_GROUPS[gi];
            if group.items().is_empty() {
                handoff_put(api, gi, ACT_GROUPS, 0);
                return AppResult::Chain(SEG_MENU);
            }
            Phase::Items(ItemsState::new(api, group, row as usize))
        }
    };
    unsafe {
        STATE.phase = phase;
    }
    AppResult::Continue
}

fn key(api: &Api, kind: u8, id: u8) -> AppResult {
    // Press and Release come around every Single on the keypad (the core drops them today; an app must not depend on it:
    // a notice raised by a Single would be cleared by the Release that follows it)
    if kind < KIND_SINGLE {
        return AppResult::Continue;
    }
    unsafe {
        // the screens change their state where it is (an ItemsState is a couple of hundred bytes: moving it in and out
        // of every handler costs more code than the handlers); a handler that leaves the screen returns the next one
        let state = &mut *core::ptr::addr_of_mut!(STATE);
        let next = match &mut state.phase {
            Phase::Groups { index } => dispatch_groups(api, kind, id, index),
            Phase::Items(st) => dispatch_items(api, kind, id, st),
            #[cfg(feature = "g_presets")]
            Phase::About { cur, back } => dispatch_about(api, kind, id, cur, *back),
            #[cfg(feature = "g_presets")]
            Phase::Grid(g) => dispatch_grid(api, kind, id, g),
            _ => None,
        };
        if let Some(phase) = next {
            state.phase = phase;
        }
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
        Phase::Items(ItemsState::new(api, SETTINGS_GROUPS[gi], 0))
    } else {
        handoff_put(api, gi, ACT_ITEMS, 0);
        Phase::Chain(seg)
    }
}

fn back_to_groups(api: &Api, gi: usize) -> Phase {
    if MY_SEG == SEG_MENU {
        Phase::Groups { index: gi }
    } else {
        handoff_put(api, gi, ACT_GROUPS, 0);
        Phase::Chain(SEG_MENU)
    }
}

fn dispatch_groups(api: &Api, kind: u8, id: u8, index: &mut usize) -> Option<Phase> {
    if kind != KIND_SINGLE && kind != KIND_REPEAT {
        return None;
    }
    let len = SETTINGS_GROUPS.len();
    match id {
        KEY_UP => *index = (*index + len - 1) % len,
        KEY_DOWN => *index = (*index + 1) % len,
        KEY_MENU if kind == KIND_SINGLE => return Some(open_group(api, *index)),
        KEY_EXIT if kind == KIND_SINGLE => return Some(Phase::Exit),
        _ => {}
    }
    None
}

fn dispatch_items(api: &Api, kind: u8, id: u8, st: &mut ItemsState) -> Option<Phase> {
    let item = st.item();
    st.note = 0;

    // VERSION and RF CHIP ID open the about view, GRID LOCATOR the locator entry: both are in segment PRESETS, which is
    // handed the row to come back to
    #[cfg(feature = "g_system")]
    if !st.editing && kind == KIND_SINGLE && id == KEY_MENU && matches!(item, SettingItem::Info | SettingItem::ChipId | SettingItem::Grid) {
        let (action, row) = match item {
            SettingItem::Grid => (ACT_GRID, 0),
            SettingItem::ChipId => (ACT_ABOUT, ABOUT_CHIP_ROW),
            _ => (ACT_ABOUT, 0),
        };
        handoff_put(api, row, action, st.index as u8);
        return Some(Phase::Chain(SEG_PRESETS));
    }

    #[cfg(feature = "g_presets")]
    if st.group == SettingsGroup::Presets {
        return dispatch_presets(api, kind, id, st);
    }

    if !EDITS_SETTINGS {
        return None;
    }

    if st.editing && kind == KIND_SINGLE && id == KEY_POUND {
        back_to_default(api, st, item);
        return None;
    }

    // after `#` a typed field has its value and no digits to wait for: it takes MENU and EXIT like a stepped one
    #[cfg(feature = "g_radio")]
    if st.editing && !st.defaulted && item == SettingItem::Offse {
        dispatch_offset_input(api, kind, id, st);
        return None;
    }
    #[cfg(feature = "g_system")]
    if st.editing && !st.defaulted && item == SettingItem::BattCal {
        dispatch_battery_input(api, kind, id, st);
        return None;
    }
    #[cfg(feature = "g_system")]
    if st.editing && !st.defaulted && matches!(item, SettingItem::Lat | SettingItem::Lon) {
        dispatch_coord_input(api, kind, id, item == SettingItem::Lat, st);
        return None;
    }

    dispatch_items_scalar(api, kind, id, item, st)
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
            } else if item != SettingItem::Reset {
                adjust(api, st, item, up);
            }
        }
        KEY_DIGIT0 if kind == KIND_SINGLE && st.editing && item.is_scalar() => {
            apply(api, st, item, scalar_floor(item));
        }
        KEY_MENU if kind == KIND_SINGLE => {
            if !st.editing {
                st.defaulted = false;
                st.snapshot = current_value(api, st, item);
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
                    _ => {}
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

// presets (segment PRESETS): several settings changed together, shown first, applied on confirmation

/// The settings a preset may change. Not in this list, so never touched by a preset: TX INHIBIT, TX BANDS, the saved
/// position, the key assignments, the battery calibration, the boot text and tune, and everything kept in the VFO.
/// A host test checks the bytes of the record.
#[cfg(feature = "g_presets")]
#[derive(Clone, Copy, PartialEq)]
enum PF {
    Sql,
    Tdr,
    Save,
    Busy,
    Tot,
    Scan,
    Beep,
    Vox,
    Tail,
    Roge,
    Abr,
    BootSnd,
}

#[cfg(feature = "g_presets")]
impl PF {
    fn name(self) -> &'static str {
        match self {
            PF::Sql => "SQUELCH",
            PF::Tdr => "DUAL WATCH",
            PF::Save => "BATTERY SAVE",
            PF::Busy => "BUSY LOCKOUT",
            PF::Tot => "TX TIMEOUT",
            PF::Scan => "SCAN RESUME",
            PF::Beep => "KEY BEEP",
            PF::Vox => "VOX",
            PF::Tail => "SQ TAIL ELIM",
            PF::Roge => "ROGER BEEP",
            PF::Abr => "BACKLIGHT",
            PF::BootSnd => "BOOT SOUND",
        }
    }

    fn get(self, s: &Settings) -> i32 {
        match self {
            PF::Sql => s.sql_level as i32,
            PF::Tdr => s.dual_standby as i32,
            PF::Save => s.save_level as i32,
            PF::Busy => s.busy_lock as i32,
            PF::Tot => s.tot_level as i32,
            PF::Scan => s.scan_mode as i32,
            PF::Beep => s.beeps_switch as i32,
            PF::Vox => s.vox_switch as i32,
            PF::Tail => s.tail_elimination as i32,
            PF::Roge => s.roger_tone as i32,
            PF::Abr => s.backlight_time as i32,
            PF::BootSnd => s.boot_sound_enabled as i32,
        }
    }

    fn set(self, s: &mut Settings, v: i32) {
        match self {
            PF::Sql => s.sql_level = v as u8,
            PF::Tdr => s.dual_standby = v != 0,
            PF::Save => s.save_level = v as u8,
            PF::Busy => s.busy_lock = v != 0,
            PF::Tot => s.tot_level = v as u8,
            PF::Scan => s.scan_mode = v as u8,
            PF::Beep => s.beeps_switch = v != 0,
            PF::Vox => s.vox_switch = v != 0,
            PF::Tail => s.tail_elimination = v != 0,
            PF::Roge => s.roger_tone = v as u8,
            PF::Abr => s.backlight_time = v as u8,
            PF::BootSnd => s.boot_sound_enabled = v != 0,
        }
    }

    /// The value in a preview row, where it shares 26 characters with the name and the value it replaces
    /// (otherwise as the settings list shows it).
    fn text(self, v: i32, out: &mut [u8]) -> usize {
        let words: &[&str] = match self {
            PF::Sql => {
                let mut w = Out::new(out);
                w.u32(v as u32, 1);
                return w.len;
            }
            PF::Tot => {
                if v == 0 {
                    return write_str(out, "OFF");
                }
                let mut w = Out::new(out);
                w.u32(v as u32 * 15, 1);
                w.str("S");
                return w.len;
            }
            PF::Save => &["OFF", "1", "2", "3", "4"],
            PF::Scan => &["TIMED", "CARRIER", "STOP"],
            PF::Roge => &["OFF", "ROGER", "MDC1200"],
            PF::Abr => &["ALWAYS", "5 SEC", "10 SEC", "15 SEC", "20 SEC"],
            _ => &["OFF", "ON"],
        };
        write_str(out, words[(v.max(0) as usize).min(words.len() - 1)])
    }
}

/// Most settings one preset changes.
#[cfg(feature = "g_presets")]
const PRESET_MAX: usize = 9;

/// In the order of the PRESETS list (QUIET, BATTERY SAVER, SCANNING, REPEATER, DEFAULTS): what each sets.
#[cfg(feature = "g_presets")]
static PRESETS: [&[(PF, u8)]; 5] = [
    // quiet: no beeps, no tone at the end of a transmission, short light
    &[(PF::Beep, 0), (PF::Roge, 0), (PF::BootSnd, 0), (PF::Abr, 1)],
    // battery saver: receiver naps (0.3 s), light short, no dual watch, no VOX
    &[(PF::Save, 3), (PF::Abr, 1), (PF::Tdr, 0), (PF::Vox, 0)],
    // scanning: stay on a call until it ends, a notch less noise, light 15 s
    &[(PF::Scan, 1), (PF::Sql, 4), (PF::Abr, 3)],
    // repeater: polite: busy lockout, no squelch crash, one minute limit, no VOX
    &[(PF::Busy, 1), (PF::Tail, 1), (PF::Tot, 4), (PF::Vox, 0)],
    // defaults: the values of a new record for everything a preset may change except the boot sound
    &[
        (PF::Sql, 3),
        (PF::Tdr, 0),
        (PF::Save, 0),
        (PF::Beep, 1),
        (PF::Abr, 2),
        (PF::Scan, 1),
        (PF::Vox, 0),
        (PF::Tail, 1),
        (PF::Roge, 0),
    ],
];

/// The settings preset `p` would change now (those not already at its value), in table order.
#[cfg(feature = "g_presets")]
fn preset_changes(s: &Settings, p: usize, out: &mut [(PF, u8); PRESET_MAX]) -> usize {
    let mut n = 0;
    for &(f, v) in PRESETS[p] {
        if f.get(s) != v as i32 {
            out[n] = (f, v);
            n += 1;
        }
    }
    n
}

#[cfg(feature = "g_presets")]
fn dispatch_presets(api: &Api, kind: u8, id: u8, st: &mut ItemsState) -> Option<Phase> {
    if st.toast {
        (api.toast)(core::ptr::null(), 0); // the list repaints over it
        st.toast = false;
    }
    if kind != KIND_SINGLE && kind != KIND_REPEAT {
        return None;
    }
    let mut ch = [(PF::Sql, 0u8); PRESET_MAX];
    let preview = st.view != 0;
    let n = if preview { preset_changes(&st.settings, st.view as usize - 1, &mut ch) } else { PRESETS.len() };
    let at = if preview { st.pidx } else { st.index };
    let moved = match id {
        KEY_UP if n > 0 => (at + n - 1) % n,
        KEY_DOWN if n > 0 => (at + 1) % n,
        _ => at,
    };
    if preview {
        st.pidx = moved;
    } else {
        st.index = moved;
    }
    match id {
        KEY_MENU if kind == KIND_SINGLE => {
            if !preview {
                st.view = st.index as u8 + 1;
                st.pidx = 0;
            } else if n > 0 {
                for &(f, v) in &ch[..n] {
                    f.set(&mut st.settings, v as i32);
                }
                preview_settings(api, &st.settings);
                commit_settings(api, &st.settings);
                st.view = 0;
                st.toast = true;
            }
        }
        KEY_EXIT if kind == KIND_SINGLE => {
            if preview {
                st.view = 0;
            } else {
                return Some(back_to_groups(api, st.group_pos()));
            }
        }
        _ => {}
    }
    None
}

/// The preview of preset `st.view - 1`: one row per setting it would change, `OLD>NEW`.
#[cfg(feature = "g_presets")]
fn draw_preview(api: &Api, st: &ItemsState) {
    let mut ch = [(PF::Sql, 0u8); PRESET_MAX];
    let p = st.view as usize - 1;
    let n = preset_changes(&st.settings, p, &mut ch);
    let sel = st.pidx.min(n.saturating_sub(1));
    let total = n.max(1);
    let top = scroll_top(total, sel);
    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        for i in 0..VISIBLE_ROWS {
            rows[i] = BLANK_ROW;
            let gi = top + i;
            if n == 0 && gi == 0 {
                write_str(&mut rows[i].label, "ALREADY SET");
            } else if gi < n {
                let (f, v) = ch[gi];
                write_str(&mut rows[i].label, f.name());
                let a = f.text(f.get(&st.settings), &mut rows[i].value);
                rows[i].value[a] = b'>';
                f.text(v as i32, &mut rows[i].value[a + 1..]);
                rows[i].has_value = true;
            }
        }
    }
    let mut title = [0u8; 26];
    let mut w = Out::new(&mut title);
    w.str(SETTINGS_GROUPS[6].items()[p].label());
    w.str(": ");
    if n == 0 {
        w.str("NO");
    } else {
        w.u32(n as u32, 1);
    }
    w.str(if n == 1 { " CHANGE" } else { " CHANGES" });
    let hint: &[u8] = if n > 0 { b"MENU apply  EXIT cancel" } else { b"EXIT back" };
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);
    (api.draw_list)(
        title.as_ptr(),
        26,
        core::ptr::addr_of!(ROWS) as *const ListRow,
        VISIBLE_ROWS as u16,
        top as u16,
        sel as u16,
        total as u16,
        false,
    );
}

// coordinates and grid locators

/// A coordinate (1e-5 degrees) as the list shows it: `40.7100N`; `----` when no position has been entered.
#[inline(never)]
fn coord_into(out: &mut [u8], v: i32, is_lat: bool) -> usize {
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

/// The six-character Maidenhead locator of a position (1e-5 degrees), field and square letters upper case and the
/// sub-square letters lower case (`EM12kx`); None when no position has been entered.
fn locator_of(lat: i32, lon: i32) -> Option<[u8; 6]> {
    if lat == COORD_NOT_SET || lon == COORD_NOT_SET {
        return None;
    }
    // 0 at the south pole and at 180 W; +90 and +180 are folded into the last square
    // (32-bit arithmetic only: the 64-bit divide costs a kilobyte on this chip)
    let lon = (lon + 18_000_000).clamp(0, 35_999_999);
    let lat = (lat + 9_000_000).clamp(0, 17_999_999);
    let (fx, rx) = (lon / 2_000_000, lon % 2_000_000); // fields are 20 x 10 degrees
    let (fy, ry) = (lat / 1_000_000, lat % 1_000_000);
    Some([
        b'A' + fx as u8,
        b'A' + fy as u8,
        b'0' + (rx / 200_000) as u8, // squares are 2 x 1 degrees
        b'0' + (ry / 100_000) as u8,
        b'a' + ((rx % 200_000) * 12 / 100_000) as u8, // sub-squares are 5 x 2.5 minutes
        b'a' + ((ry % 100_000) * 24 / 100_000) as u8,
    ])
}

// about (SYSTEM > VERSION): what this firmware and radio are, read only. Shown by segment PRESETS (SYSTEM has no room).

const ABOUT_NAME: &str = "UV-5RM OPEN FW";
const ABOUT_ROWS: usize = 7;
/// the row of the about view that shows the RF chip id (RF CHIP ID opens the view there)
const ABOUT_CHIP_ROW: usize = 5;

/// `0x` and `digits` hex digits of `v`.
#[inline(never)]
fn hex_into(out: &mut [u8], v: u32, digits: usize) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out[0] = b'0';
    out[1] = b'x';
    for i in 0..digits {
        out[2 + i] = HEX[((v >> ((digits - 1 - i) * 4)) & 0xF) as usize];
    }
    2 + digits
}

/// Row `i` of the about view. BUILD cannot say which commit this is: the core stamps its git commit into the image
/// and draws it on the boot screen, but gives apps no way to read it.
#[cfg(feature = "g_presets")]
fn about_row(api: &Api, i: usize, label: &mut [u8; 16], value: &mut [u8; 18]) {
    let (l, v): (&str, &str) = match i {
        0 => ("FIRMWARE", ABOUT_NAME),
        1 => ("BUILD", "ON BOOT SCREEN"),
        2 => ("RADIO", "UV-5RM"),
        3 => ("BOARD", "5RHNRFB-YP07"),
        4 => ("MCU", "AT32F421"),
        _ => ("", ""),
    };
    write_str(label, l);
    if i < 5 {
        write_str(value, v);
    } else if i == ABOUT_CHIP_ROW {
        write_str(label, "RF CHIP");
        hex_into(value, (api.settings_get)(7), 4);
    } else {
        write_str(label, "APP ABI");
        hex_into(value, bf5rh_abi::BUILD_HASH, 8);
    }
}

#[cfg(feature = "g_presets")]
fn draw_about(api: &Api, cur: u8) {
    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        for i in 0..VISIBLE_ROWS {
            rows[i] = BLANK_ROW;
            if i < ABOUT_ROWS {
                about_row(api, i, &mut rows[i].label, &mut rows[i].value);
                rows[i].has_value = true;
            }
        }
    }
    let hint = b"EXIT back";
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);
    let mut title = [0u8; 26];
    write_str(&mut title, "ABOUT");
    (api.draw_list)(
        title.as_ptr(),
        26,
        core::ptr::addr_of!(ROWS) as *const ListRow,
        VISIBLE_ROWS as u16,
        0,
        cur as u16,
        ABOUT_ROWS as u16,
        false,
    );
}

/// Back to the SYSTEM list, on the row it was left from.
#[cfg(feature = "g_presets")]
fn leave_view(api: &Api, back: u8) -> Phase {
    handoff_put(api, SYSTEM_GROUP, ACT_ITEMS, back);
    Phase::Chain(SEG_SYSTEM)
}

#[cfg(feature = "g_presets")]
fn dispatch_about(api: &Api, kind: u8, id: u8, cur: &mut u8, back: u8) -> Option<Phase> {
    if kind == KIND_SINGLE || kind == KIND_REPEAT {
        match id {
            KEY_UP => *cur = ((*cur as usize + ABOUT_ROWS - 1) % ABOUT_ROWS) as u8,
            KEY_DOWN => *cur = ((*cur as usize + 1) % ABOUT_ROWS) as u8,
            KEY_EXIT | KEY_MENU if kind == KIND_SINGLE => return Some(leave_view(api, back)),
            _ => {}
        }
    }
    None
}

// grid locator entry (SYSTEM > GRID LOCATOR). Shown by segment PRESETS.
//
// Six characters, `EM12kx`: two letters A-R (field), two digits (square), two letters A-X (sub-square). Letters are
// typed on the keypad the way a phone does: 2 is ABC, 3 is DEF, ..., 9 is WXYZ; the same key again steps through its
// letters, a different key (or #) accepts the letter. A key offers only the letters the place takes. Four characters
// (the square) are accepted too.

/// The letters of keys 2 to 9 are ABC, DEF, GHI, JKL, MNO, PQRS, TUV, WXYZ: the first letter of each and how many it has.
#[cfg(feature = "g_presets")]
const KEY_FIRST: [u8; 8] = *b"ADGJMPTW";
#[cfg(feature = "g_presets")]
const KEY_COUNT: [u8; 8] = [3, 3, 3, 3, 3, 4, 3, 4];

#[cfg(feature = "g_presets")]
#[derive(Clone, Copy)]
pub struct Loc {
    /// accepted characters, then the letter being chosen (upper case; shown lower case in the last two places)
    chars: [u8; 6],
    len: usize,
    /// the key whose letter is being chosen (0: none) and how many times past its first letter
    key: u8,
    tap: u8,
}

#[cfg(feature = "g_presets")]
impl Loc {
    const fn new() -> Loc {
        Loc { chars: [0; 6], len: 0, key: 0, tap: 0 }
    }

    /// The last letter place `pos` takes: A-R for the two field letters, A-X for the two sub-square letters;
    /// None for the two digits.
    fn letter_max(pos: usize) -> Option<u8> {
        match pos {
            0 | 1 => Some(b'R'),
            4 | 5 => Some(b'X'),
            _ => None,
        }
    }

    /// How many of key `k`'s letters place `pos` takes (the letters are consecutive, so it is arithmetic).
    fn offered(pos: usize, k: u8) -> u8 {
        let max = Loc::letter_max(pos).unwrap_or(0);
        if !(2..=9).contains(&k) {
            return 0;
        }
        let (first, count) = (KEY_FIRST[k as usize - 2], KEY_COUNT[k as usize - 2]);
        if max < first {
            0
        } else {
            count.min(max - first + 1)
        }
    }

    /// The `n`th letter key `k` offers.
    fn nth(k: u8, n: u8) -> u8 {
        KEY_FIRST[k as usize - 2] + n
    }

    /// Does place `pos` take key `k` (0-9)?
    fn takes(pos: usize, k: u8) -> bool {
        pos < 6
            && match Loc::letter_max(pos) {
                Some(_) => Loc::offered(pos, k) > 0,
                None => k <= 9,
            }
    }

    /// Characters entered, counting the letter being chosen.
    fn total(&self) -> usize {
        self.len + (self.key != 0) as usize
    }

    /// A digit key was pressed (0-9). Keys the place does not take are ignored.
    fn press(&mut self, k: u8) {
        if self.key != 0 {
            if k == self.key && Loc::takes(self.len, k) {
                self.tap = (self.tap + 1) % Loc::offered(self.len, k);
                self.chars[self.len] = Loc::nth(k, self.tap);
                return;
            }
            if !Loc::takes(self.len + 1, k) {
                return;
            }
            self.len += 1; // a different key accepts the letter
            self.key = 0;
        }
        if !Loc::takes(self.len, k) {
            return;
        }
        if Loc::letter_max(self.len).is_some() {
            self.key = k;
            self.tap = 0;
            self.chars[self.len] = Loc::nth(k, 0);
        } else {
            self.chars[self.len] = b'0' + k;
            self.len += 1;
        }
    }

    /// `#`: accept the letter being chosen.
    fn next(&mut self) {
        if self.key != 0 {
            self.len += 1;
            self.key = 0;
        }
    }

    /// EXIT: drop the letter being chosen, or else the last character. False when there was nothing to drop.
    fn back(&mut self) -> bool {
        if self.key != 0 {
            self.key = 0;
            true
        } else if self.len > 0 {
            self.len -= 1;
            true
        } else {
            false
        }
    }

    /// Four or six characters make a locator (the letter being chosen counts).
    fn complete(&self) -> bool {
        matches!(self.total(), 4 | 6)
    }

    /// The characters as shown: `EM12--`, sub-square letters in lower case.
    fn text(&self, out: &mut [u8; 6]) {
        for i in 0..6 {
            let c = if i < self.total() { self.chars[i] } else { b'-' };
            out[i] = if i >= 4 && c.is_ascii_uppercase() { c | 0x20 } else { c };
        }
    }

    /// The centre of the square (four characters) or sub-square (six) entered, as (latitude, longitude) in 1e-5
    /// degrees. None when the entry is not complete.
    fn center(&self) -> Option<(i32, i32)> {
        if !self.complete() {
            return None;
        }
        let c = &self.chars;
        let d = |i: usize, a: u8| (c[i] - a) as i32;
        // seconds of arc from 180 W and from 90 S
        let mut lon = d(0, b'A') * 20 * 3600 + d(2, b'0') * 2 * 3600 - 180 * 3600;
        let mut lat = d(1, b'A') * 10 * 3600 + d(3, b'0') * 3600 - 90 * 3600;
        if self.total() == 6 {
            lon += d(4, b'A') * 300 + 150;
            lat += d(5, b'A') * 150 + 75;
        } else {
            lon += 3600;
            lat += 1800;
        }
        // seconds of arc to 1e-5 degrees: * 100000 / 3600 = * 250 / 9, rounded
        let e5 = |s: i32| (s * 250 + if s >= 0 { 4 } else { -4 }) / 9;
        Some((e5(lat), e5(lon)))
    }

    /// What the keys do now, for the strip.
    fn hint(&self, note: bool) -> &'static [u8] {
        if note {
            b"Need 4 or 6 characters"
        } else if self.key != 0 {
            b"Again=next letter  # ok"
        } else if self.len == 6 {
            b"MENU ok  EXIT del"
        } else if Loc::letter_max(self.len).is_some() {
            if self.len == 4 {
                b"MENU ok  2-9 letter"
            } else {
                b"2-9 letter  EXIT del"
            }
        } else {
            b"0-9 digit  EXIT del"
        }
    }
}

#[cfg(feature = "g_presets")]
pub struct GridState {
    loc: Loc,
    /// the SYSTEM row to come back to
    back: u8,
    /// MENU was pressed on an entry that is not a locator
    note: bool,
}

#[cfg(feature = "g_presets")]
impl GridState {
    fn new(back: u8) -> GridState {
        GridState { loc: Loc::new(), back, note: false }
    }
}

#[cfg(feature = "g_presets")]
fn dispatch_grid(api: &Api, kind: u8, id: u8, g: &mut GridState) -> Option<Phase> {
    if kind != KIND_SINGLE {
        return None;
    }
    g.note = false;
    match id {
        0..=9 => g.loc.press(id),
        KEY_POUND => g.loc.next(),
        KEY_EXIT => {
            if !g.loc.back() {
                return Some(leave_view(api, g.back));
            }
        }
        KEY_MENU => {
            g.loc.next();
            match g.loc.center() {
                Some((lat, lon)) => {
                    let mut s = read_settings(api);
                    s.obs_lat = lat;
                    s.obs_lon = lon;
                    preview_settings(api, &s);
                    commit_settings(api, &s);
                    return Some(leave_view(api, g.back));
                }
                None => g.note = true,
            }
        }
        _ => {}
    }
    None
}

#[cfg(feature = "g_presets")]
fn draw_grid(api: &Api, g: &GridState) {
    unsafe {
        let rows = &mut *core::ptr::addr_of_mut!(ROWS);
        for r in rows.iter_mut() {
            *r = BLANK_ROW;
        }
        write_str(&mut rows[0].label, "LOCATOR");
        let mut t = [0u8; 6];
        g.loc.text(&mut t);
        rows[0].value[..6].copy_from_slice(&t);
        rows[0].has_value = true;
        rows[0].cursor = g.loc.len as i16; // a typed field: amber, with the cursor on the place being filled

    }
    let hint = g.loc.hint(g.note);
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);
    let mut title = [0u8; 26];
    write_str(&mut title, "GRID LOCATOR");
    (api.draw_list)(
        title.as_ptr(),
        26,
        core::ptr::addr_of!(ROWS) as *const ListRow,
        VISIBLE_ROWS as u16,
        0,
        0,
        1,
        false,
    );
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

#[inline(never)]
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

    if editing_this && !st.defaulted {
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
        Info => write_str(out, ABOUT_NAME),
        #[cfg(feature = "g_system")]
        ChipId => hex_into(out, (api.settings_get)(7), 4),
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
        Lat | Lon => coord_into(out, current_value(api, st, item), item == Lat),
        #[cfg(feature = "g_system")]
        Grid => match locator_of(st.settings.obs_lat, st.settings.obs_lon) {
            Some(t) => {
                out[..6].copy_from_slice(&t);
                6
            }
            None => write_str(out, "----"),
        },
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

        #[cfg(feature = "g_presets")]
        PresetQuiet | PresetBattery | PresetScan | PresetRepeater | PresetDefaults => 0,

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

const BLANK_ROW: ListRow = ListRow {
    label: [0; 16],
    value: [0; 18],
    has_value: false,
    cursor: -1,
};
static mut ROWS: [ListRow; VISIBLE_ROWS] = [BLANK_ROW; VISIBLE_ROWS];

fn draw(api: &Api) -> AppResult {
    let state = unsafe { &*core::ptr::addr_of!(STATE) };

    match &state.phase {
        #[cfg(feature = "g_presets")]
        Phase::About { cur, .. } => {
            draw_about(api, *cur);
            return AppResult::Continue;
        }
        #[cfg(feature = "g_presets")]
        Phase::Grid(g) => {
            draw_grid(api, g);
            return AppResult::Continue;
        }
        _ => {}
    }
    #[cfg(feature = "g_presets")]
    if let Phase::Items(st) = &state.phase {
        if st.group == SettingsGroup::Presets && st.view != 0 {
            draw_preview(api, st);
            return AppResult::Continue;
        }
    }

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
            rows[i] = BLANK_ROW;
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
                    let n = if EDITS_SETTINGS { value_text_for(api, st, gi, &mut rows[i].value) } else { 0 };
                    rows[i].has_value = n > 0;
                    // a typed field shows a text cursor after what has been typed (and so the "changing" colour)
                    if st.is_editing(gi)
                        && !st.defaulted
                        && matches!(
                            item,
                            SettingItem::Offse | SettingItem::BattCal | SettingItem::Lat | SettingItem::Lon
                        )
                    {
                        rows[i].cursor = n.min(17) as i16;
                    }
                    if st.is_editing(gi) && (matches!(item, SettingItem::Reset) || st.defaulted) {
                        rows[i].cursor = bf5rh_abi::ROW_ARMED;
                    }
                }
                _ => {}
            }
        }
    }

    let mut title_buf = [0u8; 26];
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
            _ if st.note == 1 => b"Choose TX BANDS by hand",
            SettingItem::Reset => b"HOLD # reset  EXIT cancel",
            SettingItem::Lat if !st.defaulted => b"0-9 * N/S # clear MENU ok",
            SettingItem::Lon if !st.defaulted => b"0-9 * E/W # clear MENU ok",
            SettingItem::Offse | SettingItem::BattCal if !st.defaulted => b"0-9 # default  MENU ok",
            _ => b"#default MENU ok EXIT undo",
        },
        Phase::Items(st) => {
            let item = st.item();
            let v = if EDITS_SETTINGS { current_value(api, st, item) } else { 0 };
            let n = help_into(item, v, &mut help_buf);
            &help_buf[..n]
        }
        _ => b"MENU open    EXIT back",
    };
    (api.list_hint)(hint.as_ptr(), hint.len() as u16);

    let rows_ptr = core::ptr::addr_of!(ROWS) as *const ListRow;
    (api.draw_list)(
        title_buf.as_ptr(),
        26,
        rows_ptr,
        VISIBLE_ROWS as u16,
        window_start as u16,
        selected as u16,
        total as u16,
        show_arrows,
    );

    #[cfg(feature = "g_presets")]
    if let Phase::Items(st) = &state.phase {
        if st.toast {
            (api.toast)(b"PRESET APPLIED".as_ptr(), 14);
        }
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

    /// The core's source, read when the test is built: the one other place the defaults are written down.
    const CORE_FLASH_MAP: &str = include_str!("../../src/flash_map.rs");
    const CORE_APP_MOD: &str = include_str!("../../src/app/mod.rs");
    const CORE_MAIN: &str = include_str!("../../src/main.rs");

    /// `field: value,` lines of `Settings::DEFAULT` as numbers (booleans are 0/1, COORD_NOT_SET is i32::MAX; array
    /// fields are left out).
    fn core_defaults() -> Vec<(String, i64)> {
        let start = CORE_FLASH_MAP.find("pub const DEFAULT: Settings = Settings {").expect("Settings::DEFAULT");
        let body = &CORE_FLASH_MAP[start..];
        let end = body.find("\n    };").expect("end of Settings::DEFAULT");
        let mut out = Vec::new();
        for line in body[..end].lines().skip(1) {
            let line = line.split("//").next().unwrap().trim().trim_end_matches(',');
            let Some((name, value)) = line.split_once(": ") else { continue };
            let v = match value.trim() {
                "true" => 1,
                "false" => 0,
                "COORD_NOT_SET" => i32::MAX as i64,
                n => match n.parse::<i64>() {
                    Ok(v) => v,
                    Err(_) => continue, // an array or a call
                },
            };
            out.push((name.trim().to_string(), v));
        }
        out
    }

    /// The `Settings` field behind an item (None: not a field of the record).
    fn core_field(item: SettingItem) -> Option<&'static str> {
        use SettingItem::*;
        Some(match item {
            Sql => "sql_level",
            Tot => "tot_level",
            Tdr => "dual_standby",
            Save => "save_level",
            BusyLock => "busy_lock",
            TxForbid => "tx_forbid",
            Scrm => "scramble_level",
            ScanMd => "scan_mode",
            Rit => "rit_offset",
            ChDisp => "channel_display_mode",
            Beep => "beeps_switch",
            Vox => "vox_switch",
            VoxLv => "vox_level",
            VoxDly => "vox_delay",
            Rtone => "rtone",
            Tail => "tail_elimination",
            Rptrl => "rptrl",
            Roge => "roger_tone",
            Abr => "backlight_time",
            AutoLk => "key_auto_lock",
            Side1Short => "side1_short",
            Side1Long => "side1_long",
            Side2Short => "side2_short",
            Side2Long => "side2_long",
            BandShort => "band_short",
            BandLong => "band_long",
            AniTx => "ani_tx",
            FLock => "band_lock",
            Lat => "obs_lat",
            Lon => "obs_lon",
            BattCal => "battery_cal_raw",
            BootMode => "boot_display_mode",
            BootSnd => "boot_sound_enabled",
            _ => return None,
        })
    }

    /// The app's table of defaults and the core's `Settings::DEFAULT` say the same thing for every setting the
    /// segment shows.
    #[test]
    fn defaults_match_the_core() {
        let core = core_defaults();
        assert!(core.len() >= 30, "parsed only {} fields of Settings::DEFAULT", core.len());
        for g in SETTINGS_GROUPS {
            for &item in g.items() {
                if let Some(field) = core_field(item) {
                    let want = core.iter().find(|(n, _)| n == field).unwrap_or_else(|| panic!("no core field {field}")).1;
                    assert_eq!(default_of(item), Some(want as i32), "{} (core field {field})", item.label());
                }
            }
        }
    }

    /// The items kept in the VFO rather than the record take the VFO's start-up values (step, bandwidth, power).
    #[test]
    fn vfo_defaults_match_the_core() {
        let step: i32 = CORE_APP_MOD
            .lines()
            .find_map(|l| l.trim().strip_prefix("const DEFAULT_STEP_INDEX: u8 = "))
            .expect("DEFAULT_STEP_INDEX")
            .trim_end_matches(';')
            .parse()
            .unwrap();
        assert!(CORE_MAIN.contains("const WIDE_BAND: bool = true;"));
        assert!(CORE_MAIN.contains("power: Power::Low,"));
        for g in SETTINGS_GROUPS {
            for &item in g.items() {
                match item {
                    SettingItem::Step => assert_eq!(default_of(item), Some(step)),
                    SettingItem::Wn => assert_eq!(default_of(item), Some(0)), // WIDE
                    SettingItem::TxPr => assert_eq!(default_of(item), Some(1)), // LOW
                    _ => {}
                }
            }
        }
    }

    /// Every item that holds a value has a default; the three that only show something do not.
    #[test]
    fn only_display_items_have_no_default() {
        for g in SETTINGS_GROUPS {
            for &item in g.items() {
                let shows_only = matches!(
                    item,
                    SettingItem::Info
                        | SettingItem::ChipId
                        | SettingItem::Reset
                        | SettingItem::PresetQuiet
                        | SettingItem::PresetBattery
                        | SettingItem::PresetScan
                        | SettingItem::PresetRepeater
                        | SettingItem::PresetDefaults
                        | SettingItem::Grid
                );
                assert_eq!(default_of(item).is_none(), shows_only, "{}", item.label());
            }
        }
    }

    /// Maidenhead: known squares and sub-squares, as (locator, latitude, longitude) of their centres in degrees.
    /// FN31pr is the ARRL's square in Newington CT (the sub-square spans 72.75 to 72.6667 W, 41.7083 to 41.75 N).
    #[cfg(feature = "g_presets")]
    const KNOWN: [(&str, f64, f64); 5] = [
        ("EM12kx", 32.979167, -97.125),
        ("FN31pr", 41.729167, -72.708333),
        ("JJ00aa", 0.0208333, 0.0416667),
        ("AA00aa", -89.9791667, -179.9583333),
        ("RR99xx", 89.9791667, 179.9583333),
    ];

    #[cfg(feature = "g_presets")]
    fn loc_from(text: &str) -> Loc {
        let mut l = Loc::new();
        for c in text.chars() {
            if c.is_ascii_digit() {
                l.press(c as u8 - b'0');
            } else {
                // type the letter on its key: the first press gives the key's first letter; more presses step on
                let u = c.to_ascii_uppercase() as u8;
                let i = (0..8usize).rev().find(|&i| KEY_FIRST[i] <= u).unwrap();
                let (k, n) = (i as u8 + 2, (u - KEY_FIRST[i]) as usize);
                l.press(k);
                for _ in 0..n {
                    l.press(k);
                }
                l.next();
            }
        }
        l
    }

    #[cfg(feature = "g_presets")]
    #[test]
    fn locators_have_the_right_centres() {
        for (text, lat, lon) in KNOWN {
            let (la, lo) = loc_from(text).center().expect(text);
            assert!((la as f64 / 1e5 - lat).abs() < 2e-5, "{text}: latitude {}", la as f64 / 1e5);
            assert!((lo as f64 / 1e5 - lon).abs() < 2e-5, "{text}: longitude {}", lo as f64 / 1e5);
            // and the locator of that point is the one typed
            let back = locator_of(la, lo).unwrap();
            assert_eq!(core::str::from_utf8(&back).unwrap(), text);
        }
    }

    /// Every four-character square, with a spread of sub-squares: centre then locator is the identity.
    #[cfg(feature = "g_presets")]
    #[test]
    fn locator_round_trips_through_its_centre() {
        let mut n = 0;
        for f1 in 0..18u8 {
            for f2 in 0..18u8 {
                for d1 in 0..10u8 {
                    for d2 in 0..10u8 {
                        for (s1, s2) in [(0u8, 0u8), (23, 23), (0, 23), (23, 0), (11, 12), (12, 11), ((f1 + d1) % 24, (f2 + d2) % 24)] {
                            let t = [b'A' + f1, b'A' + f2, b'0' + d1, b'0' + d2, b'A' + s1, b'A' + s2];
                            let l = Loc { chars: t, len: 6, key: 0, tap: 0 };
                            let (la, lo) = l.center().unwrap();
                            let mut want = t;
                            want[4] |= 0x20;
                            want[5] |= 0x20;
                            assert_eq!(locator_of(la, lo), Some(want), "{}", core::str::from_utf8(&t).unwrap());
                            n += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(n, 18 * 18 * 100 * 7);
    }

    /// The edges of the world fold into the last square; an unset position has no locator.
    #[cfg(feature = "g_presets")]
    #[test]
    fn locator_of_edges() {
        assert_eq!(locator_of(9_000_000, 18_000_000).map(|t| t[0]), Some(b'R'));
        assert_eq!(locator_of(-9_000_000, -18_000_000), Some(*b"AA00aa"));
        assert_eq!(locator_of(COORD_NOT_SET, 0), None);
        assert_eq!(locator_of(0, COORD_NOT_SET), None);
    }

    /// A square of four characters is accepted and set to the middle of the square.
    #[cfg(feature = "g_presets")]
    #[test]
    fn four_characters_are_a_square() {
        let l = loc_from("EM12");
        assert!(l.complete());
        let (la, lo) = l.center().unwrap();
        assert_eq!((la, lo), (3_250_000, -9_700_000));
    }

    /// Typing on the keypad: letters step on the same key; digits go straight in; bad keys are ignored.
    #[cfg(feature = "g_presets")]
    #[test]
    fn keypad_entry() {
        let mut t = [0u8; 6];
        // E is key 3 pressed twice, M is key 6 pressed once
        let mut l = Loc::new();
        l.press(3);
        l.press(3);
        l.text(&mut t);
        assert_eq!(&t, b"E-----");
        l.press(6); // a different key accepts the E
        l.text(&mut t);
        assert_eq!(&t, b"EM----");
        l.press(1);
        l.press(2);
        l.text(&mut t);
        assert_eq!(&t, b"EM12--");
        l.press(5); // K: key 5, once
        l.press(5);
        l.press(5);
        l.press(5); // J, K, L, and back to J
        l.text(&mut t);
        assert_eq!(&t, b"EM12j-");
        l.press(9); // accepts the J, then W
        l.press(9); // X
        l.press(9); // only W and X fit: back to W
        l.text(&mut t);
        assert_eq!(&t, b"EM12jw");
        assert!(l.complete());
        l.press(2); // the six places are full
        l.text(&mut t);
        assert_eq!(&t, b"EM12jw");

        // EXIT drops the letter being chosen, then the last accepted character
        assert!(l.back());
        l.text(&mut t);
        assert_eq!(&t, b"EM12j-");
        assert!(l.back());
        assert!(l.back());
        l.text(&mut t);
        assert_eq!(&t, b"EM1---");
        let mut e = Loc::new();
        assert!(!e.back());

        // bad input: the places take only what fits them
        let mut b = Loc::new();
        b.press(0); // no letters on 0
        b.press(1);
        b.press(8); // T, U, V are past R
        assert_eq!(b.total(), 0);
        b.press(7); // P, Q, R, then back to P (S is past R)
        b.press(7);
        b.press(7);
        b.press(7);
        b.text(&mut t);
        assert_eq!(&t, b"P-----");
        b.press(1); // a digit cannot follow a field letter's place: the second letter place wants a letter
        assert_eq!(b.total(), 1);
        b.press(2); // A: accepts the P, starts the second letter
        b.next();
        b.press(1);
        b.press(4);
        assert_eq!(b.len, 4);
        b.press(1); // the first sub-square place wants a letter
        assert_eq!(b.len, 4);
        b.press(8); // sub-square letter T is fine (A-X)
        b.press(8);
        b.press(8);
        b.press(8); // T, U, V, T
        b.text(&mut t);
        assert_eq!(&t, b"PA14t-");
        assert!(!b.complete()); // five characters are no locator
    }

    /// An entry that is not four or six characters is no locator.
    #[cfg(feature = "g_presets")]
    #[test]
    fn short_and_odd_entries_are_refused() {
        for text in ["", "E", "EM", "EM1", "EM12j"] {
            let l = loc_from(text);
            assert!(!l.complete(), "{text}");
            assert!(l.center().is_none(), "{text}");
        }
        for text in ["EM12", "EM12jw"] {
            assert!(loc_from(text).center().is_some(), "{text}");
        }
    }

    /// Every hint fits the 26-character strip.
    #[cfg(feature = "g_presets")]
    #[test]
    fn grid_hints_fit() {
        for text in ["", "E", "EM", "EM1", "EM12", "EM12j", "EM12jw"] {
            let l = loc_from(text);
            for note in [false, true] {
                assert!(l.hint(note).len() <= 26, "{text}");
            }
            let mut p = l;
            p.press(5);
            assert!(p.hint(false).len() <= 26);
        }
    }

    /// The handoff buffer round-trips, and an old three-byte buffer reads as row 0.
    #[test]
    fn handoff_round_trips() {
        for group in 0..GROUP_SEG.len() {
            for action in [ACT_GROUPS, ACT_ITEMS, ACT_ABOUT, ACT_GRID] {
                for row in [0u8, 3, 9, 255] {
                    assert_eq!(handoff_decode(&handoff_encode(group, action, row), 4), Some((group, action, row)));
                }
            }
        }
        let old = [HANDOFF_MAGIC, 5, ACT_ITEMS, 77];
        assert_eq!(handoff_decode(&old, 3), Some((5, ACT_ITEMS, 0)));
        assert_eq!(handoff_decode(&old, 2), None);
        assert_eq!(handoff_decode(&[0, 5, 1, 0], 4), None);
        assert_eq!(handoff_decode(&[HANDOFF_MAGIC, GROUP_SEG.len() as u8, 1, 0], 4), None);
    }

    /// Where each preset-able setting lives in the 168-byte record.
    #[cfg(feature = "g_presets")]
    fn pf_byte(f: PF) -> usize {
        match f {
            PF::Sql => 0,
            PF::Tail => 1,
            PF::Busy => 2,
            PF::Tdr => 5,
            PF::Vox => 6,
            PF::Tot => 8,
            PF::Beep => 9,
            PF::Roge => 10,
            PF::Scan => 13,
            PF::Save => 15,
            PF::Abr => 16,
            PF::BootSnd => 31,
        }
    }

    #[cfg(feature = "g_presets")]
    fn pf_core_field(f: PF) -> &'static str {
        match f {
            PF::Sql => "sql_level",
            PF::Tail => "tail_elimination",
            PF::Busy => "busy_lock",
            PF::Tdr => "dual_standby",
            PF::Vox => "vox_switch",
            PF::Tot => "tot_level",
            PF::Beep => "beeps_switch",
            PF::Roge => "roger_tone",
            PF::Scan => "scan_mode",
            PF::Save => "save_level",
            PF::Abr => "backlight_time",
            PF::BootSnd => "boot_sound_enabled",
        }
    }

    #[cfg(feature = "g_presets")]
    fn pf_max(f: PF) -> i32 {
        match f {
            PF::Sql => 9,
            PF::Tot => 12,
            PF::Save | PF::Abr => 4,
            PF::Scan | PF::Roge => 2,
            _ => 1,
        }
    }

    /// A settings record with a different, in-range value in every byte a preset might touch and in every byte it
    /// must not (including the two transmit restrictions, the position, the keys and the calibration).
    #[cfg(feature = "g_presets")]
    fn sample_record(seed: u8) -> [u8; SETTINGS_BYTES] {
        let mut b = [0u8; SETTINGS_BYTES];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(31).wrapping_add(seed) % 2;
        }
        b[0] = seed % 10; // squelch
        b[8] = seed % 13; // tx timeout
        b[10] = seed % 3; // roger
        b[13] = seed % 3; // scan
        b[15] = seed % 5; // battery save
        b[16] = seed % 5; // backlight
        b[3] = 0; // TX INHIBIT off: the preset must not turn it on or off
        b[29] = 9; // TX BANDS = ALL
        b[32..40].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        b[21] = 0x34;
        b[22] = 0x12;
        b
    }

    /// A preset changes only the settings it lists: every other byte of the record (TX INHIBIT, TX BANDS, the
    /// position, the key assignments, the battery calibration, the boot text and tune) comes out as it went in.
    #[cfg(feature = "g_presets")]
    #[test]
    fn presets_change_only_what_they_list() {
        for p in 0..PRESETS.len() {
            for seed in 0..40u8 {
                let before = sample_record(seed);
                let mut s = Settings::from_bytes(&before);
                for &(f, v) in PRESETS[p] {
                    f.set(&mut s, v as i32);
                }
                let after = s.to_bytes();
                for i in 0..SETTINGS_BYTES {
                    if after[i] != before[i] {
                        assert!(PRESETS[p].iter().any(|&(f, _)| pf_byte(f) == i), "preset {p} changed byte {i}");
                    }
                }
                for must_keep in [3, 29, 21, 22, 23, 24, 25, 26, 27, 28] {
                    assert_eq!(after[must_keep], before[must_keep], "preset {p} touched byte {must_keep}");
                }
                assert_eq!(&after[32..], &before[32..], "preset {p}");
            }
        }
    }

    /// Applying a preset leaves nothing for it to change; every value is one the setting can have.
    #[cfg(feature = "g_presets")]
    #[test]
    fn presets_are_valid_and_idempotent() {
        for p in 0..PRESETS.len() {
            let mut s = Settings::from_bytes(&sample_record(7));
            for &(f, v) in PRESETS[p] {
                assert!((v as i32) <= pf_max(f), "{}: {v} out of range", f.name());
                f.set(&mut s, v as i32);
            }
            let mut ch = [(PF::Sql, 0u8); PRESET_MAX];
            assert_eq!(preset_changes(&s, p, &mut ch), 0, "preset {p} not idempotent");
            assert!(PRESETS[p].len() <= PRESET_MAX);
            // no setting listed twice
            for (i, &(a, _)) in PRESETS[p].iter().enumerate() {
                for &(b, _) in &PRESETS[p][i + 1..] {
                    assert!(a != b, "preset {p} lists {} twice", a.name());
                }
            }
        }
    }

    /// DEFAULTS sets what the core's `Settings::DEFAULT` holds.
    #[cfg(feature = "g_presets")]
    #[test]
    fn defaults_preset_matches_the_core() {
        let core = core_defaults();
        for &(f, v) in PRESETS[4] {
            let want = core.iter().find(|(n, _)| n == pf_core_field(f)).expect("core field").1;
            assert_eq!(v as i64, want, "{}", f.name());
        }
    }

    /// A preview row is `OLD>NEW` in 18 characters, and its label in 16, for any pair of values.
    #[cfg(feature = "g_presets")]
    #[test]
    fn preview_rows_fit() {
        for f in [
            PF::Sql, PF::Tdr, PF::Save, PF::Busy, PF::Tot, PF::Scan, PF::Beep, PF::Vox, PF::Tail, PF::Roge, PF::Abr,
            PF::BootSnd,
        ] {
            assert!(f.name().len() <= 16);
            for a in 0..=pf_max(f) {
                for b in (0..=pf_max(f)).filter(|&b| b != a) {
                    let (mut x, mut y) = ([0u8; 32], [0u8; 32]);
                    let (n, m) = (f.text(a, &mut x), f.text(b, &mut y));
                    assert!(n > 0 && m > 0 && n + 1 + m <= 18, "{} {a}>{b} is {} characters", f.name(), n + 1 + m);
                    assert!(f.name().len() + n + 1 + m <= 24, "{} {a}>{b} crowds its label", f.name());
                }
            }
        }
    }

    /// `#` never loosens a transmit restriction: TX INHIBIT goes to ON from anywhere; TX BANDS is restored only
    /// from FCC HAM or ALL (which allow everything FCC HAM does), never from a narrower choice (CE HAM, GB HAM,
    /// PMR446, GMRS/FRS, CA HAM) nor from NONE (no transmitting at all) nor from the two 137-174 choices, which
    /// lack some FCC HAM bands.
    #[test]
    fn default_never_loosens_a_transmit_restriction() {
        for cur in 0..=1 {
            assert!(default_allowed(SettingItem::TxForbid, cur));
        }
        for cur in 0..=9 {
            assert_eq!(default_allowed(SettingItem::FLock, cur), cur == 1 || cur == 9, "TX BANDS {cur}");
        }
        #[cfg(feature = "g_radio")]
        assert_eq!(default_of(SettingItem::TxForbid), Some(1), "TX INHIBIT defaults to ON");
        #[cfg(feature = "g_system")]
        assert_eq!(default_of(SettingItem::FLock), Some(1), "TX BANDS defaults to FCC HAM");
    }
}
