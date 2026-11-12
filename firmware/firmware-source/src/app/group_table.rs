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
