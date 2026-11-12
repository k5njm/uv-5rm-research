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

/// In order of use to a ham. The index is the group's number: BAND then that digit selects it (0 = ALL; groups past 9
/// have no digit). Slot ranges are the upstream channel plan's, plus 351-380, 381-410 and 411-420 for the new groups.
pub(crate) const MODES: [ChanMode; 16] = [
    ChanMode { name: "ALL", short: "ALL", ranges: ALL },
    ChanMode { name: "REPEATERS", short: "RPT", ranges: &[(234, 304), (327, 344)] },
    ChanMode { name: "SIMPLEX", short: "SMPLX", ranges: &[(351, 380)] },
    ChanMode { name: "GMRS", short: "GMRS", ranges: &[(305, 344)] },
    ChanMode { name: "FRS", short: "FRS", ranges: &[(305, 326)] },
    ChanMode { name: "MURS", short: "MURS", ranges: &[(345, 349)] },
    ChanMode { name: "SATELLITES", short: "SAT", ranges: &[(381, 410)] },
    ChanMode { name: "SONDE", short: "SONDE", ranges: &[(411, 420)] },
    ChanMode { name: "WEATHER", short: "WX", ranges: &[(1, 7)] },
    ChanMode { name: "AIR TRAFFIC", short: "AIR", ranges: &[(168, 233)] },
    ChanMode { name: "RAILROAD", short: "RAIL", ranges: &[(158, 167)] },
    ChanMode { name: "POLICE FIRE EMS", short: "PD/FD", ranges: &[(14, 105)] },
    ChanMode { name: "FED & GOV", short: "FED", ranges: &[(106, 146)] },
    ChanMode { name: "UTILITIES", short: "UTIL", ranges: &[(147, 157)] },
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
/// listen to. Scanning from the VFO sweeps the band the frequency is in, not the synthesizer's whole range. (No FM
/// broadcast entry: that is the FM app's tuner chip, outside `RX_BANDS`.)
pub(crate) const SCAN_BANDS: [(u32, u32, &str); 8] = [
    (118_000_000, 137_000_000, "AIR"),
    (144_000_000, 148_000_000, "2M"),
    (151_820_000, 154_600_000, "MURS"),
    (156_000_000, 162_025_000, "MARINE"),
    (162_400_000, 162_550_000, "WX"),
    (222_000_000, 225_000_000, "1.25M"),
    (420_000_000, 450_000_000, "70CM"),
    (462_550_000, 467_725_000, "GMRS"),
];

/// The receive band `freq_hz` is in.
pub(crate) fn rx_band_of(freq_hz: u32) -> Option<(u32, u32)> {
    RX_BANDS.iter().copied().find(|&(lo, hi)| (lo..=hi).contains(&freq_hz))
}

/// The band to scan from `freq_hz`: the listed band it is in, otherwise the 1 MHz it is in; always inside the receive
/// band it belongs to. A frequency the receiver cannot hear is first moved to the nearest receivable one (`snap_rx`).
pub(crate) fn scan_band(freq_hz: u32) -> (u32, u32) {
    let f = snap_rx(freq_hz);
    let (rlo, rhi) = rx_band_of(f).unwrap_or((f, f));
    for &(lo, hi, _) in SCAN_BANDS.iter() {
        let (lo, hi) = (lo.max(rlo), hi.min(rhi));
        if lo < hi && (lo..=hi).contains(&f) {
            return (lo, hi);
        }
    }
    let lo = f / 1_000_000 * 1_000_000;
    (lo.max(rlo), (lo + 1_000_000).min(rhi))
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

/// What the receiver covers (Hz, inclusive), from the radio's published ranges: the air band (AM), VHF, 1.25 m and
/// UHF. The synthesizer tunes far wider (16-560 and 740-1120 MHz), but outside these the front end hears nothing.
/// FM broadcast (76-108 MHz) is a separate tuner chip, reached through the FM app, not the VFO.
pub(crate) const RX_BANDS: [(u32, u32); 3] = [
    (108_000_000, 174_000_000),
    (200_000_000, 260_000_000),
    (350_000_000, 520_000_000),
];
/// What the FM app tunes: a frequency typed in this range on the main screen opens it there.
pub(crate) const FM_BROADCAST: (u32, u32) = (87_500_000, 108_000_000);

pub(crate) fn rx_allowed(freq_hz: u32) -> bool {
    RX_BANDS.iter().any(|&(lo, hi)| (lo..=hi).contains(&freq_hz))
}

/// `RX_BANDS` clipped to `lo..=hi`, ascending; a band with nothing inside comes back as `(1, 0)` (empty).
fn rx_segments(lo: u32, hi: u32) -> [(u32, u32); 3] {
    let mut out = [(1, 0); 3];
    for (o, &(a, b)) in out.iter_mut().zip(RX_BANDS.iter()) {
        let (a, b) = (a.max(lo), b.min(hi));
        if a <= b {
            *o = (a, b);
        }
    }
    out
}

/// The next frequency a step of `step_hz` from `freq_hz` can land on, staying inside `lo..=hi` and inside `RX_BANDS`:
/// the plain step when that is receivable; otherwise the nearest edge of the next band in that direction; past the last
/// band (up) or before the first (down) it wraps to the other end. `None` when the range holds nothing receivable.
/// A VFO step is `next_rx(f, step, up, 0, u32::MAX)`; a scan passes its band.
pub(crate) fn next_rx(freq_hz: u32, step_hz: u32, up: bool, lo: u32, hi: u32) -> Option<u32> {
    let segs = rx_segments(lo, hi);
    if up {
        let cand = freq_hz.saturating_add(step_hz);
        for &(a, b) in segs.iter().filter(|s| s.0 <= s.1) {
            if cand <= b {
                return Some(cand.max(a));
            }
        }
        segs.iter().find(|s| s.0 <= s.1).map(|s| s.0)
    } else {
        let cand = freq_hz.checked_sub(step_hz);
        for &(a, b) in segs.iter().rev().filter(|s| s.0 <= s.1) {
            if let Some(c) = cand {
                if c >= a {
                    return Some(c.min(b));
                }
            }
        }
        segs.iter().rev().find(|s| s.0 <= s.1).map(|s| s.1)
    }
}

/// One UP/DOWN step in the VFO.
pub(crate) fn step_rx(freq_hz: u32, step_hz: u32, up: bool) -> u32 {
    next_rx(freq_hz, step_hz, up, 0, u32::MAX).unwrap_or(freq_hz)
}

/// The lowest receivable frequency in `lo..=hi`, if any.
pub(crate) fn first_rx_in(lo: u32, hi: u32) -> Option<u32> {
    rx_segments(lo, hi).iter().find(|s| s.0 <= s.1).map(|s| s.0)
}

/// `freq_hz` if the receiver covers it, else the nearest edge of a receive band (the upper one on a tie).
pub(crate) fn snap_rx(freq_hz: u32) -> u32 {
    if rx_allowed(freq_hz) {
        return freq_hz;
    }
    let mut best = (u32::MAX, freq_hz);
    for &(a, b) in RX_BANDS.iter() {
        for edge in [a, b] {
            let d = edge.abs_diff(freq_hz);
            if d < best.0 || (d == best.0 && edge > best.1) {
                best = (d, edge);
            }
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_in_a_group_counts_from_one_across_its_ranges() {
        assert_eq!(MODES[1].name, "REPEATERS");
        assert_eq!(group_pos(1, 233), Some(1)); // REPEATERS: printed 234 is its first channel
        assert_eq!(group_pos(1, 303), Some(71)); // printed 304, last of the first range
        assert_eq!(group_pos(1, 326), Some(72)); // printed 327, first of the second range
        assert_eq!(group_pos(1, 0), None);
        assert_eq!(group_pos(0, 233), Some(234)); // ALL: the printed number
        assert_eq!(MODES[8].name, "WEATHER");
        assert_eq!(group_pos(8, 0), Some(1)); // WEATHER
        assert_eq!(group_pos(7, 410), Some(1)); // SONDE starts at printed 411
        assert_eq!(group_pos(2, 350), Some(1)); // SIMPLEX starts at printed 351
        assert_eq!(group_pos(6, 380), Some(1)); // SATELLITES starts at printed 381
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
        assert_eq!(group_slot(1, 90), None); // REPEATERS has 89 slots
    }

    #[test]
    fn a_vfo_scan_stays_in_its_band() {
        assert_eq!(scan_band(146_520_000), (144_000_000, 148_000_000));
        assert_eq!(scan_band(446_000_000), (420_000_000, 450_000_000));
        assert_eq!(scan_band(162_550_000), (162_400_000, 162_550_000));
        assert_eq!(scan_band(300_250_000), (260_000_000, 260_000_000)); // not receivable: snapped to 260 MHz
        assert_eq!(scan_band(400_250_000), (400_000_000, 401_000_000)); // no listed band: the megahertz it is in
        assert_eq!(band_name(146_520_000), "2M");
        assert_eq!(band_name(300_250_000), "");
    }

    #[test]
    fn a_scan_band_never_leaves_the_receive_bands() {
        // the 108 MHz edge is the air-band side of VHF, not the FM broadcast band
        assert_eq!(scan_band(108_000_000), (108_000_000, 109_000_000));
        assert_eq!(scan_band(174_000_000), (174_000_000, 174_000_000)); // clipped to the one edge frequency
        assert_eq!(scan_band(173_500_000), (173_000_000, 174_000_000));
        assert_eq!(scan_band(200_000_000), (200_000_000, 201_000_000));
        assert_eq!(scan_band(260_000_000), (260_000_000, 260_000_000));
        assert_eq!(scan_band(350_000_000), (350_000_000, 351_000_000));
        assert_eq!(scan_band(520_000_000), (520_000_000, 520_000_000));
        // a stale or odd frequency is snapped first
        assert_eq!(scan_band(100_000_000), (108_000_000, 109_000_000));
        assert_eq!(scan_band(900_000_000), (520_000_000, 520_000_000));
        // across the whole synthesizer range the band is receivable at both ends and inside one receive band
        let mut f = 0u32;
        while f <= 1_200_000_000 {
            let (lo, hi) = scan_band(f);
            let band = rx_band_of(lo).expect("a scan band starts receivable");
            assert!(lo <= hi && hi <= band.1, "{f}: {lo}..{hi}");
            assert!(rx_allowed(hi), "{f}");
            f += 997_000;
        }
    }

    #[test]
    fn typed_vfo_frequencies_are_limited_to_what_the_receiver_covers() {
        for ok in [118_000_000, 146_520_000, 223_500_000, 404_800_000, 446_000_000, 520_000_000] {
            assert!(rx_allowed(ok), "{ok}");
        }
        for no in [16_000_000, 50_000_000, 100_100_000, 107_999_999, 300_000_000, 560_000_000, 900_000_000] {
            assert!(!rx_allowed(no), "{no}");
        }
    }

    const A: (u32, u32) = RX_BANDS[0];
    const B: (u32, u32) = RX_BANDS[1];
    const C: (u32, u32) = RX_BANDS[2];

    #[test]
    fn a_step_inside_a_band_is_the_plain_step() {
        assert_eq!(step_rx(146_520_000, 25_000, true), 146_545_000);
        assert_eq!(step_rx(146_520_000, 25_000, false), 146_495_000);
        assert_eq!(step_rx(A.0, 12_500, true), A.0 + 12_500);
        assert_eq!(step_rx(C.1, 12_500, false), C.1 - 12_500);
        // landing exactly on an edge is allowed
        assert_eq!(step_rx(A.1 - 25_000, 25_000, true), A.1);
        assert_eq!(step_rx(B.0 + 25_000, 25_000, false), B.0);
        assert_eq!(step_rx(C.0 + 25_000, 25_000, false), C.0);
    }

    #[test]
    fn stepping_up_off_the_top_of_a_band_lands_on_the_bottom_of_the_next() {
        assert_eq!(step_rx(A.1, 12_500, true), B.0);
        assert_eq!(step_rx(A.1 - 5_000, 12_500, true), B.0); // a step that overshoots the edge
        assert_eq!(step_rx(B.1, 25_000, true), C.0);
        assert_eq!(step_rx(B.1 - 1, 100_000, true), C.0);
        // from the gap itself (a stale frequency)
        assert_eq!(step_rx(180_000_000, 25_000, true), B.0);
        assert_eq!(step_rx(300_000_000, 25_000, true), C.0);
    }

    #[test]
    fn stepping_down_off_the_bottom_of_a_band_lands_on_the_top_of_the_previous() {
        assert_eq!(step_rx(C.0, 12_500, false), B.1);
        assert_eq!(step_rx(C.0 + 5_000, 12_500, false), B.1);
        assert_eq!(step_rx(B.0, 25_000, false), A.1);
        assert_eq!(step_rx(B.0 + 1, 100_000, false), A.1);
        assert_eq!(step_rx(180_000_000, 25_000, false), A.1);
        assert_eq!(step_rx(300_000_000, 25_000, false), B.1);
    }

    #[test]
    fn stepping_wraps_from_the_top_of_the_last_band_to_the_bottom_of_the_first_and_back() {
        assert_eq!(step_rx(C.1, 12_500, true), A.0);
        assert_eq!(step_rx(C.1 - 5_000, 12_500, true), A.0);
        assert_eq!(step_rx(A.0, 12_500, false), C.1);
        assert_eq!(step_rx(A.0 + 5_000, 12_500, false), C.1);
        // beyond the whole range: up from above the last band wraps; below the first band, up goes to its bottom edge
        assert_eq!(step_rx(700_000_000, 25_000, true), A.0);
        assert_eq!(step_rx(100_000_000, 25_000, false), C.1);
        assert_eq!(step_rx(100_000_000, 25_000, true), A.0);
        assert_eq!(step_rx(0, 25_000, false), C.1); // nothing below zero
        assert_eq!(step_rx(u32::MAX, 25_000, true), A.0); // no overflow
    }

    #[test]
    fn a_step_always_lands_on_a_receivable_frequency_and_a_lap_comes_back_around() {
        for step in [1_000, 5_000, 6_250, 12_500, 25_000, 50_000, 100_000, 1_000_000, 10_000_000] {
            for f in (0..1_200_000_000u32).step_by(3_137_000) {
                for up in [true, false] {
                    let n = step_rx(f, step, up);
                    assert!(rx_allowed(n), "{f} step {step} up {up} -> {n}");
                }
            }
        }
        for step in [100_000, 250_000, 1_000_000, 10_000_000] {
            let mut f = A.0;
            let mut hops = 0u32;
            loop {
                f = step_rx(f, step, true);
                assert!(rx_allowed(f), "{f}");
                hops += 1;
                if f == A.0 {
                    break;
                }
                assert!(hops < 10_000, "a lap with step {step} never closed");
            }
        }
    }

    #[test]
    fn a_scan_inside_its_band_wraps_within_it_and_skips_the_gaps_between_bands() {
        // a band: 144-148 MHz
        assert_eq!(next_rx(147_990_000, 25_000, true, 144_000_000, 148_000_000), Some(144_000_000));
        assert_eq!(next_rx(144_000_000, 25_000, false, 144_000_000, 148_000_000), Some(148_000_000));
        assert_eq!(next_rx(146_000_000, 25_000, true, 144_000_000, 148_000_000), Some(146_025_000));
        // a custom range across the gap between VHF and 1.25 m: 170-210 MHz holds 170-174 and 200-210
        assert_eq!(next_rx(173_990_000, 25_000, true, 170_000_000, 210_000_000), Some(200_000_000));
        assert_eq!(next_rx(209_990_000, 25_000, true, 170_000_000, 210_000_000), Some(170_000_000));
        assert_eq!(next_rx(200_000_000, 25_000, false, 170_000_000, 210_000_000), Some(174_000_000));
        assert_eq!(next_rx(170_000_000, 25_000, false, 170_000_000, 210_000_000), Some(210_000_000));
        // a range that holds nothing receivable
        assert_eq!(next_rx(180_000_000, 25_000, true, 175_000_000, 199_000_000), None);
        assert_eq!(next_rx(300_000_000, 25_000, false, 262_000_000, 340_000_000), None);
        assert_eq!(first_rx_in(175_000_000, 199_000_000), None);
        // a range only partly receivable starts at its first receivable frequency
        assert_eq!(first_rx_in(100_000_000, 130_000_000), Some(108_000_000));
        assert_eq!(first_rx_in(170_000_000, 210_000_000), Some(170_000_000));
        assert_eq!(first_rx_in(174_000_001, 210_000_000), Some(200_000_000));
    }

    #[test]
    fn a_scan_sweep_of_every_kind_of_range_in_both_directions_never_tunes_outside() {
        let ranges = [
            (87_500_000, 108_000_000),
            (100_000_000, 130_000_000),
            (144_000_000, 148_000_000),
            (170_000_000, 210_000_000),
            (255_000_000, 360_000_000),
            (500_000_000, 560_000_000),
            (16_000_000, 1_100_000_000),
        ];
        for (lo, hi) in ranges {
            for up in [true, false] {
                let mut f = first_rx_in(lo, hi).unwrap();
                for _ in 0..2_000 {
                    f = next_rx(f, 250_000, up, lo, hi).unwrap();
                    assert!(rx_allowed(f) && (lo..=hi).contains(&f), "{lo}..{hi} up {up}: {f}");
                }
            }
        }
    }

    #[test]
    fn snapping_goes_to_the_nearest_edge_of_a_receive_band() {
        assert_eq!(snap_rx(146_520_000), 146_520_000);
        assert_eq!(snap_rx(100_000_000), A.0);
        assert_eq!(snap_rx(176_000_000), A.1); // 2 MHz above VHF, 24 below 1.25 m
        assert_eq!(snap_rx(190_000_000), B.0); // 16 above VHF, 10 below 1.25 m
        assert_eq!(snap_rx(700_000_000), C.1);
        assert_eq!(snap_rx(0), A.0);
    }
}

