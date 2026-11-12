use super::{scan, App};
use cortex_m::peripheral::SYST;

// TODO: FM radio was made as app, add key function to app
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyFunction {
    None,
    WideNarrow,
    Monitor,
    Mode,
    TxTone,
    Scan,
    Power,
    Flashlight,
    Reverse,
    /// Open the overlay app in this launcher slot (0-based). Nothing happens if the slot is empty or invalid.
    App(u8),
}

/// Largest stored key-function value: 11..=19 open the app in launcher slot 1..=9.
pub const KEYFN_MAX: u8 = 19;
const KEYFN_APP_BASE: u8 = 11;

impl KeyFunction {
    pub fn label(self) -> &'static str {
        match self {
            KeyFunction::None => "NONE",
            KeyFunction::WideNarrow => "WIDE/NAR",
            KeyFunction::Monitor => "MONITOR",
            KeyFunction::Mode => "MODE",
            KeyFunction::TxTone => "TX TONE",
            KeyFunction::Scan => "SCAN",
            KeyFunction::Power => "POWER",
            KeyFunction::Flashlight => "LIGHT",
            KeyFunction::Reverse => "REVERSE",
            KeyFunction::App(_) => "APP",
        }
    }
}

pub(super) fn from_u8(v: u8) -> KeyFunction {
    match v {
        1 => KeyFunction::WideNarrow,
        2 => KeyFunction::Monitor,
        3 => KeyFunction::Mode,
        4 => KeyFunction::TxTone,
        // 5=> fm radio
        6 => KeyFunction::Scan,
        7 => KeyFunction::Power,
        8 => KeyFunction::Flashlight,
        // 9 => search (now app)
        10 => KeyFunction::Reverse,
        KEYFN_APP_BASE..=KEYFN_MAX => KeyFunction::App(v - KEYFN_APP_BASE),
        _ => KeyFunction::None,
    }
}

pub(super) fn invoke(app: &mut App, syst: &mut SYST, func: KeyFunction) {
    match func {
        KeyFunction::None | KeyFunction::TxTone => {}
        KeyFunction::WideNarrow => app.toggle_wide_narrow(syst),
        KeyFunction::Monitor => app.radio.toggle_monitor(),
        KeyFunction::Mode => app.toggle_modulation(syst),
        // TODO: fm radio
        KeyFunction::Scan => scan::enter(app, syst),
        KeyFunction::Power => app.toggle_power(syst),
        KeyFunction::Flashlight => app.flashlight.toggle(),
        // TODO: search
        KeyFunction::Reverse => app.toggle_reverse(syst),
        KeyFunction::App(slot) => {
            if super::overlay::slot_valid(app, slot) {
                super::overlay::enter(app, syst, slot);
            }
        }
    }
}
