//! What the main screen shows of the battery, and when the low-battery warning is due: pure integer code, no hardware
//! types, shared with the host simulator's unit tests (`scripts/dev.sh unit`). The caller (`App::poll_battery`) feeds it a
//! 12-bit ADC reading and whether the radio is transmitting.
//!
//! Two exponential averages, stepped every `STEP_100US` (50 ms) of real time, whatever the main loop's rate:
//!   - `slow` (the resting value, time constant 3.2 s) follows the reading only while receiving. It is frozen while the
//!     radio transmits, so a long transmission never drags it down: the warning reads it, and it is what the screen shows
//!     when not transmitting;
//!   - `fast` (time constant 0.2 s) starts from `slow` at the moment of keying and follows the sag; it is what the screen
//!     shows while transmitting. When the key is released the screen goes straight back to `slow` (the pack's voltage
//!     does the same: the sag is the load, not lost charge).
//! The shown value only moves when the average is more than 4 LSB away from it (1 LSB is 0.3 cV at the default
//! calibration). The bar count is computed from the shown value with the calibration thresholds, as it always was.
//!
//! The warning (`LowBattery`) reads only the resting value, only while not transmitting. Its percentages are rough:
//! `PACK` models a 2-cell Li-ion pack by voltage under a light load; the core compares centivolts against the three
//! thresholds the table gives (`WARN_CV`, `CRIT_CV`, `REARM_CV`), computed at compile time.

/// Fixed-point bits of the averages (12-bit reading << 10 fits an i32 with room to spare).
const Q: u32 = 10;
pub(crate) const STEP_100US: u32 = 500;
/// Most steps run in one call: a long stall does not stretch the time constant, but one call stays short.
const MAX_STEPS: u32 = 20;
const SHIFT_RX: u32 = 6;
const SHIFT_TX: u32 = 2;
const HYST: i32 = 4;

pub(crate) struct BatteryFilter {
    slow: i32,
    fast: i32,
    shown: u16,
    was_tx: bool,
    last: u32,
}

/// Bars from the 8-bit reading and the calibration thresholds (`cal[2..=5]`), as the main screen always has.
fn bars_for(raw8: u8, cal: &[u8; 6]) -> u8 {
    (raw8 > cal[2]) as u8 + (raw8 > cal[3]) as u8 + (raw8 > cal[4]) as u8 + (raw8 > cal[5]) as u8
}

impl BatteryFilter {
    pub(crate) const fn new() -> Self {
        BatteryFilter { slow: 0, fast: 0, shown: 0, was_tx: false, last: 0 }
    }

    /// The first reading: both averages start there.
    pub(crate) fn init(&mut self, raw12: u16, now: u32) {
        let raw12 = raw12.min(4095);
        self.slow = (raw12 as i32) << Q;
        self.fast = self.slow;
        self.shown = raw12;
        self.was_tx = false;
        self.last = now;
    }

    /// Feed the latest reading (any call rate; it acts once per `STEP_100US` of `now`, in 100 us units).
    pub(crate) fn update(&mut self, now: u32, raw12: u16, tx: bool) {
        if tx && !self.was_tx {
            self.fast = self.slow;
        }
        self.was_tx = tx;
        let steps = now.wrapping_sub(self.last) / STEP_100US;
        if steps == 0 {
            return;
        }
        self.last = if steps > MAX_STEPS { now } else { self.last.wrapping_add(steps * STEP_100US) };
        let target = (raw12.min(4095) as i32) << Q;
        for _ in 0..steps.min(MAX_STEPS) {
            if tx {
                self.fast += (target - self.fast) >> SHIFT_TX;
            } else {
                self.slow += (target - self.slow) >> SHIFT_RX;
            }
        }
        let avg = (if tx { self.fast } else { self.slow } + (1 << (Q - 1))) >> Q;
        if (avg - self.shown as i32).abs() > HYST {
            self.shown = avg as u16;
        }
    }

    /// What the screen shows (12-bit).
    pub(crate) fn raw12(&self) -> u16 {
        self.shown
    }

    /// The resting reading (12-bit): never moved by a transmission.
    pub(crate) fn resting(&self) -> u16 {
        ((self.slow + (1 << (Q - 1))) >> Q) as u16
    }

    pub(crate) fn bars(&self, cal: &[u8; 6]) -> u8 {
        bars_for((self.shown >> 4) as u8, cal)
    }
}

/// Rough charge of a 2-cell Li-ion pack from its resting voltage in centivolts: (centivolts, percent), rising.
/// Per cell: 3.10 V 0% (the owner's choice, above the 3.0 V a cell is usually cut at), 3.50 V 10%, 3.65 V 20%,
/// 3.85 V 50%, 4.05 V 80%, 4.20 V 100%: the shape of a typical Li-ion discharge curve at a light load, with the steep
/// knee near empty. An approximation by voltage alone: the load, temperature and age move it by 10 points or more.
pub(crate) const PACK: [(u16, u8); 6] = [(620, 0), (700, 10), (730, 20), (770, 50), (810, 80), (840, 100)];

/// The voltage at which the table reaches `pct` (compile time).
pub(crate) const fn cv_at(pct: u8) -> u16 {
    let mut i = 1;
    while i < PACK.len() - 1 && PACK[i].1 < pct {
        i += 1;
    }
    let (v0, p0) = PACK[i - 1];
    let (v1, p1) = PACK[i];
    v0 + ((v1 - v0) as u32 * (pct - p0) as u32 / (p1 - p0) as u32) as u16
}

/// The percentage for a voltage (tests and tools: the core only compares against the thresholds).
#[cfg(test)]
pub(crate) fn percent(cv: u32) -> u8 {
    if cv <= PACK[0].0 as u32 {
        return 0;
    }
    for w in PACK.windows(2) {
        if cv < w[1].0 as u32 {
            let (v0, p0) = (w[0].0 as u32, w[0].1 as u32);
            let (v1, p1) = (w[1].0 as u32, w[1].1 as u32);
            return (p0 + (cv - v0) * (p1 - p0) / (v1 - v0)) as u8;
        }
    }
    100
}

/// The first warning: 20% (7.30 V).
pub(crate) const WARN_CV: u32 = cv_at(20) as u32;
/// Below this (10%, 7.00 V) the warning repeats.
pub(crate) const CRIT_CV: u32 = cv_at(10) as u32;
/// The warning re-arms above this: 0.15 V over the first threshold, 7.45 V (about 33%), far outside the noise of the
/// resting reading (a few cV at most) and the rebound after a transmission, but passed when a charger is connected.
pub(crate) const REARM_CV: u32 = WARN_CV + 15;
/// Repeat interval below `CRIT_CV`: 10 minutes in 100 us units.
pub(crate) const REPEAT_100US: u32 = 6_000_000;

const OWE_CHIME: u8 = 1;
const OWE_NOTE: u8 = 2;

/// The warning: `stage` 0 armed, 1 warned at 20%, 2 warned below 10%; `owed` what has not been delivered yet (the chime
/// waits while a signal is heard, the notice for the main screen); `at` when the last chime played.
pub(crate) struct LowBattery {
    stage: u8,
    owed: u8,
    at: u32,
}

impl LowBattery {
    pub(crate) const fn new() -> Self {
        LowBattery { stage: 0, owed: 0, at: 0 }
    }

    /// Judge the resting voltage (centivolts). Call only while not transmitting.
    pub(crate) fn step(&mut self, now: u32, cv: u32) {
        let fire = if cv >= REARM_CV {
            self.stage = 0;
            false
        } else if cv < CRIT_CV {
            let f = self.stage < 2 || now.wrapping_sub(self.at) >= REPEAT_100US;
            self.stage = 2;
            f
        } else if cv < WARN_CV && self.stage == 0 {
            self.stage = 1;
            true
        } else {
            false
        };
        if fire {
            self.owed = OWE_CHIME | OWE_NOTE;
        }
    }

    pub(crate) fn chime_due(&self) -> bool {
        self.owed & OWE_CHIME != 0
    }

    /// The chime has been played (or is muted): start the repeat interval.
    pub(crate) fn chimed(&mut self, now: u32) {
        self.owed &= !OWE_CHIME;
        self.at = now;
    }

    pub(crate) fn note_due(&self) -> bool {
        self.owed & OWE_NOTE != 0
    }

    pub(crate) fn noted(&mut self) {
        self.owed &= !OWE_NOTE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAL: [u8; 6] = [0, 0, 140, 148, 152, 156];
    /// Counts per centivolt at the default calibration (2537 counts at 7.60 V).
    fn raw_of(cv: u32) -> u16 {
        (cv * 2537 / 760) as u16
    }
    fn cv_of(raw: u16) -> u32 {
        raw as u32 * 760 / 2537
    }

    fn started(raw: u16) -> (BatteryFilter, u32) {
        let mut f = BatteryFilter::new();
        f.init(raw, 1000);
        (f, 1000)
    }

    /// Run `ms` milliseconds of the main loop at one call per 10 ms with a constant reading.
    fn run(f: &mut BatteryFilter, now: &mut u32, ms: u32, raw: u16, tx: bool) {
        for _ in 0..ms / 10 {
            *now = now.wrapping_add(100);
            f.update(*now, raw, tx);
        }
    }

    // ---- the display filter ----

    #[test]
    fn first_reading_is_the_start() {
        let (f, _) = started(2537);
        assert_eq!(f.raw12(), 2537);
        assert_eq!(f.resting(), 2537);
        let (f, _) = started(0);
        assert_eq!((f.raw12(), f.bars(&CAL)), (0, 0));
        let (f, _) = started(4095);
        assert_eq!((f.raw12(), f.bars(&CAL)), (4095, 4));
    }

    #[test]
    fn receiving_moves_over_seconds() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 1000, 2400, false);
        assert!(f.raw12() > 2600 - 60, "after 1 s {}", f.raw12());
        run(&mut f, &mut now, 2200, 2400, false);
        let at3 = f.raw12();
        assert!(at3 < 2600 - 100 && at3 > 2600 - 140, "after 3.2 s {}", at3);
        run(&mut f, &mut now, 7000, 2400, false);
        assert!(f.raw12() <= 2420, "after 10 s {}", f.raw12());
        run(&mut f, &mut now, 20000, 2400, false);
        assert!(f.raw12() <= 2404 && f.raw12() >= 2396);
    }

    #[test]
    fn sag_shows_within_a_second_of_keying() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 500, 2600, false);
        run(&mut f, &mut now, 1000, 2400, true);
        assert!(f.raw12() <= 2410, "{}", f.raw12());
        run(&mut f, &mut now, 300, 2400, true);
        assert!(f.raw12() <= 2403);
    }

    #[test]
    fn unkeying_returns_to_the_resting_value_and_a_long_transmission_does_not_move_it() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 5000, 2600, false);
        run(&mut f, &mut now, 600_000, 2300, true); // ten minutes keyed, heavy sag
        assert!(f.raw12() <= 2304);
        assert_eq!(f.resting(), 2600, "the resting value is frozen while keyed");
        run(&mut f, &mut now, 100, 2600, false);
        assert!(f.raw12() >= 2590, "the screen is back at once: {}", f.raw12());
        assert!(f.resting() >= 2597);
    }

    #[test]
    fn noise_never_changes_the_value() {
        let mut seed: u32 = 12345;
        for centre in [2537u16, 2560, 2300, 3000, 2400] {
            let (mut f, mut now) = started(centre);
            let v0 = f.raw12();
            for i in 0..20_000u32 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 16) % 7;
                let raw = centre + noise as u16 - 3;
                now = now.wrapping_add(30);
                f.update(now, if i % 2 == 0 { raw } else { centre + (i % 3) as u16 - 1 }, false);
                assert_eq!(f.raw12(), v0);
            }
        }
    }

    #[test]
    fn bars_follow_the_shown_value() {
        let (f, _) = started(2400); // raw8 150: above 148, below 152
        assert_eq!(f.bars(&CAL), 2);
        let (f, _) = started(2000);
        assert_eq!(f.bars(&CAL), 0);
        let (f, _) = started(2600); // raw8 162
        assert_eq!(f.bars(&CAL), 4);
    }

    #[test]
    fn extremes_do_not_overflow() {
        let (mut f, mut now) = started(4095);
        run(&mut f, &mut now, 30_000, 0, false);
        assert!(f.raw12() <= 4);
        run(&mut f, &mut now, 30_000, 4095, true);
        assert!(f.raw12() >= 4090);
        run(&mut f, &mut now, 3_000, 65535, false);
        assert!(f.raw12() <= 4095);
        let (mut f, mut now) = started(0);
        run(&mut f, &mut now, 30_000, 4095, false);
        assert!(f.raw12() >= 4090 && f.bars(&CAL) == 4);
    }

    #[test]
    fn the_clock_wrapping_and_a_stalled_loop_are_harmless() {
        let mut f = BatteryFilter::new();
        f.init(2500, u32::MAX - 100);
        let mut now = u32::MAX - 100;
        run(&mut f, &mut now, 3000, 2500, false);
        assert_eq!(f.raw12(), 2500);
        now = now.wrapping_add(100_000);
        f.update(now, 2300, false);
        assert!(f.raw12() > 2300 + 100);
    }

    // ---- the percentage table ----

    #[test]
    fn the_table_at_its_points_and_between() {
        for (cv, pct) in PACK {
            assert_eq!(percent(cv as u32), pct, "{} cV", cv);
        }
        assert_eq!(percent(0), 0);
        assert_eq!(percent(619), 0);
        assert_eq!(percent(1000), 100);
        assert_eq!(percent(u32::MAX), 100);
        assert_eq!(percent(660), 5); // half way from 0% to 10%
        assert_eq!(percent(750), 35);
        assert_eq!(percent(790), 65);
        let mut last = 0;
        for cv in 600..900 {
            let p = percent(cv);
            assert!(p >= last, "monotonic at {}", cv);
            last = p;
        }
    }

    #[test]
    fn the_thresholds_come_from_the_table() {
        assert_eq!(WARN_CV, 730);
        assert_eq!(CRIT_CV, 700);
        assert_eq!(REARM_CV, 745);
        assert_eq!(percent(WARN_CV), 20);
        assert_eq!(percent(CRIT_CV), 10);
        assert_eq!(cv_at(0), 620);
        assert_eq!(cv_at(100), 840);
        assert_eq!(cv_at(5), 660);
        assert_eq!(cv_at(35), 750);
        assert!(percent(REARM_CV) >= 30 && percent(REARM_CV) <= 35);
    }

    // ---- the warning ----

    struct Rig {
        f: BatteryFilter,
        w: LowBattery,
        now: u32,
        alerts: u32,
    }

    impl Rig {
        fn new(cv: u32) -> Rig {
            let (f, now) = started(raw_of(cv));
            Rig { f, w: LowBattery::new(), now, alerts: 0 }
        }
        /// Like `App::poll_battery` and its delivery: the chime plays when due.
        fn tick(&mut self, raw: u16, tx: bool, heard: bool) {
            self.now = self.now.wrapping_add(100);
            self.f.update(self.now, raw, tx);
            if !tx {
                self.w.step(self.now, cv_of(self.f.resting()) as u32);
                if self.w.chime_due() && !heard {
                    self.w.chimed(self.now);
                    self.w.noted();
                    self.alerts += 1;
                }
            }
        }
        fn run(&mut self, ms: u32, cv: u32, tx: bool) {
            for _ in 0..ms / 10 {
                self.tick(raw_of(cv), tx, false);
            }
        }
    }

    #[test]
    fn a_slow_discharge_crossing_20_percent_warns_exactly_once() {
        let mut r = Rig::new(780);
        for cv in (735..=760).rev() {
            r.run(10_000, cv, false);
        }
        assert_eq!(r.alerts, 0);
        for cv in (715..=734).rev() {
            r.run(10_000, cv, false);
        }
        assert_eq!(r.alerts, 1, "20% crossed once");
        r.run(300_000, 715, false);
        assert_eq!(r.alerts, 1);
    }

    #[test]
    fn hovering_around_the_threshold_with_noise_does_not_repeat() {
        let mut r = Rig::new(740);
        let mut seed = 7u32;
        for _ in 0..200_000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let cv = 730 + (seed >> 16) % 9; // 7.30 .. 7.38 V, noise of 8 cV
            r.tick(raw_of(cv - 4), false, false);
        }
        assert_eq!(r.alerts, 1, "{}", r.alerts);
    }

    #[test]
    fn charging_above_the_rearm_point_then_discharging_warns_again() {
        let mut r = Rig::new(725);
        r.run(20_000, 725, false);
        assert_eq!(r.alerts, 1);
        r.run(60_000, 744, false); // not clear of the threshold yet
        r.run(60_000, 725, false);
        assert_eq!(r.alerts, 1);
        r.run(60_000, 760, false); // charged
        r.run(60_000, 725, false);
        assert_eq!(r.alerts, 2);
    }

    #[test]
    fn below_10_percent_repeats_every_10_minutes_and_stops_when_it_rises() {
        let mut r = Rig::new(690);
        r.run(10_000, 690, false);
        assert_eq!(r.alerts, 1, "at power-on below 10%: one alert");
        r.run(590_000, 690, false);
        assert_eq!(r.alerts, 1, "not before 10 minutes");
        r.run(20_000, 690, false);
        assert_eq!(r.alerts, 2);
        r.run(1_200_000, 690, false);
        assert_eq!(r.alerts, 4);
        r.run(1_800_000, 715, false); // up between the thresholds: no more
        assert_eq!(r.alerts, 4);
    }

    #[test]
    fn crossing_10_percent_after_20_warns_again() {
        let mut r = Rig::new(780);
        r.run(20_000, 725, false);
        assert_eq!(r.alerts, 1);
        r.run(60_000, 695, false);
        assert_eq!(r.alerts, 2);
    }

    #[test]
    fn a_long_transmission_with_heavy_sag_on_a_30_percent_pack_does_not_warn() {
        let mut r = Rig::new(750); // about 35%
        r.run(30_000, 750, false);
        for _ in 0..3 {
            r.run(600_000, 650, true); // ten minutes keyed at 6.50 V
            assert_eq!(r.alerts, 0, "during");
            r.run(5_000, 748, false);
            assert_eq!(r.alerts, 0, "right after");
        }
        r.run(120_000, 748, false);
        assert_eq!(r.alerts, 0);
        // the sag alone, with rests in between
        let mut r = Rig::new(745);
        r.run(30_000, 745, false);
        for _ in 0..20 {
            r.run(30_000, 690, true);
            r.run(3_000, 744, false);
        }
        assert_eq!(r.alerts, 0);
    }

    #[test]
    fn a_chime_held_while_a_signal_is_heard_is_delivered_when_it_ends() {
        let mut r = Rig::new(780);
        r.run(5_000, 780, false);
        for _ in 0..3000 {
            r.tick(raw_of(720), false, true);
        }
        assert_eq!(r.alerts, 0, "held while heard");
        assert!(r.w.chime_due());
        r.tick(raw_of(720), false, false);
        assert_eq!(r.alerts, 1);
        assert!(!r.w.chime_due());
    }

    #[test]
    fn power_on_below_20_warns_once() {
        let mut r = Rig::new(720);
        r.run(5_000, 720, false);
        assert_eq!(r.alerts, 1);
        r.run(1_000_000, 720, false);
        assert_eq!(r.alerts, 1);
        let mut r = Rig::new(735); // above 20%: quiet
        r.run(60_000, 735, false);
        assert_eq!(r.alerts, 0);
    }

    #[test]
    fn the_repeat_counter_wraps_cleanly() {
        let mut w = LowBattery::new();
        let start = u32::MAX - 1_000_000;
        w.step(start, 690);
        assert!(w.chime_due());
        w.chimed(start);
        w.step(start.wrapping_add(5_999_999), 690);
        assert!(!w.chime_due());
        w.step(start.wrapping_add(6_000_000), 690); // across the wrap
        assert!(w.chime_due());
    }

    #[test]
    fn calibration_extremes() {
        let mut w = LowBattery::new();
        w.step(0, 0); // a dead or missing reading
        assert!(w.chime_due());
        let mut w = LowBattery::new();
        w.step(0, u32::MAX);
        assert!(!w.chime_due() && !w.note_due());
        // the calibrated voltage of a full scale reading at the extremes of the stored calibration, as the core computes it
        let cv = |raw: u32, cal: u32| raw * 760 / cal.max(1);
        assert_eq!(cv(0, 2537), 0);
        assert!(cv(4095, 1) > REARM_CV); // 4095 * 760 does not overflow a u32
        assert!(cv(4095, 65535) < CRIT_CV);
    }
}
