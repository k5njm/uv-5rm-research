//! The channel groups: pure data and the membership test, shared with the host simulator (no hardware types here).
//! Membership is by channel number today; apps reach it only through `Api::chan_in_group`, so it can become a
//! per-channel tag later without changing them (docs/CHANNEL-GROUPS.md).

pub(crate) struct ChanMode {
    pub name: &'static str,
    pub short: &'static str,
    /// Inclusive channel numbers as printed on the reference sheet (from 1).
    pub(crate) ranges: &'static [(u16, u16)],
}

/// Every slot: printed numbers 1..=1000.
pub(crate) const ALL: &[(u16, u16)] = &[(1, 1000)];

pub(crate) const MODES: [ChanMode; 13] = [
    ChanMode { name: "ALL", short: "ALL", ranges: ALL },
    ChanMode { name: "WEATHER", short: "WX", ranges: &[(1, 7)] },
    ChanMode { name: "POLICE FIRE EMS", short: "PD/FD", ranges: &[(14, 105)] },
    ChanMode { name: "FED & GOV", short: "FED", ranges: &[(106, 146)] },
    ChanMode { name: "UTILITIES", short: "UTIL", ranges: &[(147, 157)] },
    ChanMode { name: "RAILROAD", short: "RAIL", ranges: &[(158, 167)] },
    ChanMode { name: "AIR TRAFFIC", short: "AIR", ranges: &[(168, 233)] },
    ChanMode { name: "REPEATERS", short: "RPT", ranges: &[(234, 304), (327, 344)] },
    ChanMode { name: "FRS", short: "FRS", ranges: &[(305, 326)] },
    ChanMode { name: "GMRS", short: "GMRS", ranges: &[(305, 344)] },
    ChanMode { name: "MURS", short: "MURS", ranges: &[(345, 349)] },
    ChanMode { name: "FRS GMRS MURS", short: "F/G/M", ranges: &[(305, 349)] },
    // Channel list stays open here; the scan itself sweeps the typed range.
    ChanMode { name: "CUSTOM RANGE", short: "RANGE", ranges: ALL },
];

/// `num` is a flash slot, counting from 0.
pub(crate) fn contains(mode: u8, num: u16) -> bool {
    let printed = num + 1;
    MODES[mode as usize]
        .ranges
        .iter()
        .any(|&(a, b)| (a..=b).contains(&printed))
}

/// Where slot `num` (from 0) sits in group `mode`, counting from 1 across the group's ranges: K5RWK in slot 233 is
/// channel 01 of REPEATERS. In ALL this is the printed channel number. `None` if the slot is not in the group.
pub(crate) fn group_pos(mode: u8, num: u16) -> Option<u16> {
    let printed = num + 1;
    let mut before = 0u16;
    for &(a, b) in MODES[mode as usize].ranges {
        if (a..=b).contains(&printed) {
            return Some(before + printed - a + 1);
        }
        before += b - a + 1;
    }
    None
}

/// The slot (from 0) at position `pos` (from 1) of group `mode`: the inverse of `group_pos`.
pub(crate) fn group_slot(mode: u8, pos: u16) -> Option<u16> {
    let mut left = pos.checked_sub(1)?;
    for &(a, b) in MODES[mode as usize].ranges {
        let len = b - a + 1;
        if left < len {
            return Some(a + left - 1);
        }
        left -= len;
    }
    None
}

/// Bands a VFO scan stays inside (Hz, inclusive): the amateur bands this radio covers and the services people
/// listen to. Scanning from the VFO sweeps the band the frequency is in, not the synthesizer's whole range.
pub(crate) const SCAN_BANDS: [(u32, u32, &str); 9] = [
    (87_500_000, 108_000_000, "FM"),
    (118_000_000, 137_000_000, "AIR"),
    (144_000_000, 148_000_000, "2M"),
    (151_820_000, 154_600_000, "MURS"),
    (156_000_000, 162_025_000, "MARINE"),
    (162_400_000, 162_550_000, "WX"),
    (222_000_000, 225_000_000, "1.25M"),
    (420_000_000, 450_000_000, "70CM"),
    (462_550_000, 467_725_000, "GMRS"),
];

/// The band to scan from `freq_hz`: the listed band it is in, otherwise the 1 MHz it is in.
pub(crate) fn scan_band(freq_hz: u32) -> (u32, u32) {
    for &(lo, hi, _) in SCAN_BANDS.iter() {
        if (lo..=hi).contains(&freq_hz) {
            return (lo, hi);
        }
    }
    let lo = freq_hz / 1_000_000 * 1_000_000;
    (lo, lo + 1_000_000)
}

/// Short name of the listed band `freq_hz` is in ("2M"); empty outside them. Shown in the status bar in VFO mode,
/// where a channel group's name would mean nothing.
pub(crate) fn band_name(freq_hz: u32) -> &'static str {
    for &(lo, hi, name) in SCAN_BANDS.iter() {
        if (lo..=hi).contains(&freq_hz) {
            return name;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_in_a_group_counts_from_one_across_its_ranges() {
        assert_eq!(group_pos(7, 233), Some(1)); // REPEATERS: printed 234 is its first channel
        assert_eq!(group_pos(7, 303), Some(71)); // printed 304, last of the first range
        assert_eq!(group_pos(7, 326), Some(72)); // printed 327, first of the second range
        assert_eq!(group_pos(7, 0), None);
        assert_eq!(group_pos(0, 233), Some(234)); // ALL: the printed number
        assert_eq!(group_pos(1, 0), Some(1)); // WEATHER
    }

    #[test]
    fn slot_is_the_inverse_of_position_for_every_group() {
        for mode in 0..MODES.len() as u8 {
            for num in 0..1000u16 {
                if let Some(pos) = group_pos(mode, num) {
                    assert_eq!(group_slot(mode, pos), Some(num), "group {mode} slot {num}");
                }
            }
            assert_eq!(group_slot(mode, 0), None);
        }
        assert_eq!(group_slot(7, 90), None); // REPEATERS has 89 slots
    }

    #[test]
    fn a_vfo_scan_stays_in_its_band() {
        assert_eq!(scan_band(146_520_000), (144_000_000, 148_000_000));
        assert_eq!(scan_band(446_000_000), (420_000_000, 450_000_000));
        assert_eq!(scan_band(162_550_000), (162_400_000, 162_550_000));
        assert_eq!(scan_band(300_250_000), (300_000_000, 301_000_000)); // no listed band: the megahertz it is in
        assert_eq!(band_name(146_520_000), "2M");
        assert_eq!(band_name(300_250_000), "");
    }
}

