use super::modes;
use cortex_m::peripheral::SYST;

use crate::device::radio::{Modulation, Power};

use super::side;
use super::{App, ChVfoMode, MAX_CHANNEL_NUM, NO_CHANNELS_HOLD_TICKS};

pub(super) fn push_watching_config(app: &mut App) {
    let s = &app.sides[app.watching];
    app.radio.set_frequency(s.cfg.freq_hz);
    app.radio.set_tx_frequency(s.cfg.tx_freq_hz);
    app.radio.set_power(s.cfg.power);
    app.radio.set_subaudio_tx(s.cfg.subaudio_tx);
    app.radio.set_subaudio_rx(s.cfg.subaudio_rx);
    app.radio.set_modulation(s.cfg.modulation);
}

pub(super) fn apply_watching_to_radio(app: &mut App, syst: &mut SYST) {
    push_watching_config(app);
    if !app.transmitting {
        app.radio.enter_rx(syst);
    }
}

pub(super) fn sync_watching_to_master(app: &mut App, syst: &mut SYST) {
    app.watching = app.master;
    apply_watching_to_radio(app, syst);
}

pub(super) fn step(app: &mut App, syst: &mut SYST, up: bool) {
    let s = &mut app.sides[app.master];
    match s.vfo_chan {
        ChVfoMode::Vfo => {
            let step_hz = s.step_deci_hz() * 10;
            s.rx_freq_hz = if up {
                s.rx_freq_hz.saturating_add(step_hz)
            } else {
                s.rx_freq_hz.saturating_sub(step_hz)
            };
            s.auto_modulation();
            s.refresh_cfg_freqs();
            app.save_vfo();
        }
        ChVfoMode::Channel => {
            let current = s.channel_num;
            if let Some(next) = find_programmed_channel(app, current, up) {
                load_channel_num(app, next);
            }
        }
    }
    sync_watching_to_master(app, syst);
}

pub(super) fn find_programmed_channel(
    app: &mut App,
    from: u16,
    up: bool,
) -> Option<u16> {
    let mut num = from;
    for _ in 0..=MAX_CHANNEL_NUM {
        num = if up {
            if num >= MAX_CHANNEL_NUM {
                0
            } else {
                num + 1
            }
        } else if num == 0 {
            MAX_CHANNEL_NUM
        } else {
            num - 1
        };
        if !app.storage.is_channel_empty(num)
            && modes::contains(app.chan_mode, num)
        {
            return Some(num);
        }
    }
    None
}

pub(super) fn commit_input(app: &mut App, syst: &mut SYST) {
    let value = app.input.entered_value();
    match app.sides[app.master].vfo_chan {
        ChVfoMode::Channel => {
            // A typed number is the one the screen shows: the position in the current group, from 1
            // (in ALL, the printed channel number).
            let slot = u16::try_from(value)
                .ok()
                .and_then(|pos| super::group_table::group_slot(app.chan_mode, pos));
            if let Some(num) = slot {
                if num <= MAX_CHANNEL_NUM && !app.storage.is_channel_empty(num) {
                    load_channel_num(app, num);
                    sync_watching_to_master(app, syst);
                }
            }
        }
        ChVfoMode::Vfo => {
            // Only what the receiver covers. The synthesizer would tune almost anything, which looked as if the
            // radio could receive there; FM broadcast is a different tuner, reached through the FM app.
            let hz = value.saturating_mul(1000);
            if super::group_table::rx_allowed(hz) {
                let s = &mut app.sides[app.master];
                s.rx_freq_hz = hz;
                s.auto_modulation();
                s.refresh_cfg_freqs();
                app.save_vfo();
                sync_watching_to_master(app, syst);
            } else {
                // A broadcast frequency opens the FM app on it (EXIT comes back here).
                let fm = super::group_table::FM_BROADCAST;
                if !((fm.0..=fm.1).contains(&hz) && super::overlay::enter_fm(app, syst, (value / 100) as u16)) {
                    app.notice_kind = super::NOTICE_OUT_OF_RANGE;
                    app.no_channels_ticks = NO_CHANNELS_HOLD_TICKS;
                }
            }
        }
    }
    app.input.clear();
}

pub(super) fn toggle_vfo_channel(app: &mut App, syst: &mut SYST) {
    let s = &mut app.sides[app.master];
    match s.vfo_chan {
        ChVfoMode::Vfo => {
            s.vfo_backup = side::VfoBackup {
                rx_freq_hz: s.rx_freq_hz,
                freq_dir: s.freq_dir,
                offset_hz: s.offset_hz,
                wide_band: s.cfg.wide_band,
                power: s.cfg.power,
                subaudio_tx: s.cfg.subaudio_tx,
                subaudio_rx: s.cfg.subaudio_rx,
                ani_target: s.ani_target,
            };
            let num = s.channel_num;
            let num = if app.storage.is_channel_empty(num)
                || !modes::contains(app.chan_mode, num)
            {
                find_programmed_channel(app, num, true)
            } else {
                Some(num)
            };
            if let Some(num) = num {
                load_channel_num(app, num);
            } else {
                app.notice_kind = super::NOTICE_NO_CHANNELS;
                app.no_channels_ticks = NO_CHANNELS_HOLD_TICKS;
            }
        }
        ChVfoMode::Channel => {
            s.restore_vfo_from_backup();
        }
    }

    app.input.clear();
    app.ani_target_override = None;
    sync_watching_to_master(app, syst);
}

pub(super) fn switch_side(app: &mut App, syst: &mut SYST) {
    app.master = 1 - app.master;
    // each side has its own channel group: scanning and stepping follow the side that now has the keys
    app.chan_mode = app.side_groups[app.master];
    app.input.clear();
    app.ani_target_override = None;
    sync_watching_to_master(app, syst);
}

pub(super) fn toggle_reverse(app: &mut App, syst: &mut SYST) {
    let s = &mut app.sides[app.master];
    s.reversed = !s.reversed;
    s.refresh_cfg_freqs();
    sync_watching_to_master(app, syst);
}

pub(super) fn toggle_power(app: &mut App, syst: &mut SYST) {
    let s = &mut app.sides[app.master];
    s.cfg.power = match s.cfg.power {
        Power::High => Power::Low,
        Power::Low => Power::Mid,
        Power::Mid => Power::High,
    };
    commit_side_change(app, syst);
}

pub(super) fn toggle_wide_narrow(app: &mut App, syst: &mut SYST) {
    let s = &mut app.sides[app.master];
    s.cfg.wide_band = !s.cfg.wide_band;
    commit_side_change(app, syst);
}

pub(super) fn toggle_modulation(app: &mut App, syst: &mut SYST) {
    let s = &mut app.sides[app.master];
    s.cfg.modulation = match s.cfg.modulation {
        Modulation::Fm => Modulation::Am,
        Modulation::Am => Modulation::Usb,
        // no cw and cwf here
        Modulation::Usb | Modulation::Cw | Modulation::Cwf => Modulation::Fm,
    };
    sync_watching_to_master(app, syst);
}

pub(super) fn commit_side_change(app: &mut App, syst: &mut SYST) {
    app.sides[app.master].refresh_cfg_freqs();
    if matches!(app.sides[app.master].vfo_chan, ChVfoMode::Vfo) {
        app.save_vfo();
    }
    sync_watching_to_master(app, syst);
}

pub(super) fn load_channel_num(app: &mut App, num: u16) {
    let ch = app.storage.read_channel(num);
    app.sides[app.master].load_channel(num, &ch);
    app.ani_target_override = None;
}

pub(super) fn refresh_channel_display(app: &mut App, num: u16) {
    for i in 0..app.sides.len() {
        let showing = app.sides[i].vfo_chan == ChVfoMode::Channel
            && app.sides[i].channel_num == num;
        if !showing {
            continue;
        }
        if app.storage.is_channel_empty(num) {
            if let Some(next) = find_programmed_channel(app, num, true) {
                let ch = app.storage.read_channel(next);
                app.sides[i].load_channel(next, &ch);
            } else {
                app.sides[i].restore_vfo_from_backup();
                app.notice_kind = super::NOTICE_NO_CHANNELS;
                app.no_channels_ticks = NO_CHANNELS_HOLD_TICKS;
            }
        } else {
            let ch = app.storage.read_channel(num);
            app.sides[i].load_channel(num, &ch);
        }
    }
}
