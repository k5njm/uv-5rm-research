//! What the main screen shows of the battery: pure integer code, no hardware types, shared with the host simulator's
//! unit tests (`scripts/dev.sh unit`). The caller (`App::poll_battery`) feeds it a 12-bit ADC reading and whether the
//! radio is transmitting; it keeps the value and the bar count that are drawn.
//!
//! The reading is an exponential average stepped every `STEP_100US` (50 ms) of real time, whatever the main loop's rate:
//!   - receiving: time constant 3.2 s (1/64 per step), so a change shows over some seconds (90% after 7.4 s);
//!   - transmitting: time constant 0.2 s (1/4 per step), so the sag under load is on screen within a second;
//!   - the 2 s after the key is released: time constant 0.8 s (1/16 per step), a blend back to the resting value
//!     instead of a jump, then the slow average again.
//! The shown value only moves when the average is more than 4 LSB away from it (2 LSB while transmitting or recovering),
//! so noise of a few LSB never changes a digit; at the default calibration 1 LSB is 0.3 cV. The bar count has one
//! 8-bit step (16 LSB) of hysteresis around each threshold. The first reading starts the average, so there is no ramp up
//! from zero. Nothing in the firmware acts on this value: there is no low-battery warning, beep, TX inhibit or
//! power-off threshold in the core (the screen is its only consumer; apps and BATTERY CAL use the raw ring average).

/// Fixed-point bits of the average (12-bit reading << 10 fits an i32 with room to spare).
const Q: u32 = 10;
pub(crate) const STEP_100US: u32 = 500;
/// Most steps run in one call: a long stall does not stretch the time constant, but one call stays short.
const MAX_STEPS: u32 = 20;
const SHIFT_RX: u32 = 6;
const SHIFT_TX: u32 = 2;
const SHIFT_RECOVER: u32 = 4;
/// Steps of the blend after unkeying (2 s).
pub(crate) const RECOVER_STEPS: u8 = 40;
const HYST_RX: i32 = 4;
const HYST_TX: i32 = 2;

pub(crate) struct BatteryFilter {
    acc: i32,
    shown: u16,
    bars: u8,
    recover: u8,
    was_tx: bool,
    last: u32,
}

/// Bars from the 8-bit reading and the calibration thresholds (`cal[2..=5]`), as the main screen always has.
fn bars_for(raw8: u8, cal: &[u8; 6]) -> u8 {
    if raw8 > cal[5] {
        4
    } else if raw8 > cal[4] {
        3
    } else if raw8 > cal[3] {
        2
    } else if raw8 > cal[2] {
        1
    } else {
        0
    }
}

impl BatteryFilter {
    pub(crate) const fn new() -> Self {
        BatteryFilter { acc: 0, shown: 0, bars: 4, recover: 0, was_tx: false, last: 0 }
    }

    /// The first reading: the average starts there.
    pub(crate) fn init(&mut self, raw12: u16, now: u32, cal: &[u8; 6]) {
        let raw12 = raw12.min(4095);
        self.acc = (raw12 as i32) << Q;
        self.shown = raw12;
        self.bars = bars_for((raw12 >> 4) as u8, cal);
        self.recover = 0;
        self.was_tx = false;
        self.last = now;
    }

    /// Feed the latest reading (any call rate; it acts once per `STEP_100US` of `now`, in 100 us units).
    pub(crate) fn update(&mut self, now: u32, raw12: u16, tx: bool, cal: &[u8; 6]) {
        if tx {
            self.recover = 0;
        } else if self.was_tx {
            self.recover = RECOVER_STEPS;
        }
        self.was_tx = tx;
        let elapsed = now.wrapping_sub(self.last);
        let steps = elapsed / STEP_100US;
        if steps == 0 {
            return;
        }
        self.last = if steps > MAX_STEPS { now } else { self.last.wrapping_add(steps * STEP_100US) };
        let target = (raw12.min(4095) as i32) << Q;
        for _ in 0..steps.min(MAX_STEPS) {
            let shift = if tx {
                SHIFT_TX
            } else if self.recover > 0 {
                self.recover -= 1;
                SHIFT_RECOVER
            } else {
                SHIFT_RX
            };
            self.acc += (target - self.acc) >> shift;
        }
        let avg = ((self.acc + (1 << (Q - 1))) >> Q) as i32;
        let hyst = if tx || self.recover > 0 { HYST_TX } else { HYST_RX };
        if (avg - self.shown as i32).abs() > hyst {
            self.shown = avg as u16;
        }
        let raw8 = (self.shown >> 4) as u8;
        let up = bars_for(raw8.saturating_sub(1), cal);
        let down = bars_for(raw8.saturating_add(1), cal);
        if up > self.bars {
            self.bars = up;
        } else if down < self.bars {
            self.bars = down;
        }
    }

    pub(crate) fn raw12(&self) -> u16 {
        self.shown
    }

    pub(crate) fn bars(&self) -> u8 {
        self.bars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAL: [u8; 6] = [0, 0, 140, 148, 152, 156];

    fn started(raw: u16) -> (BatteryFilter, u32) {
        let mut f = BatteryFilter::new();
        f.init(raw, 1000, &CAL);
        (f, 1000)
    }

    /// Run `ms` milliseconds of the main loop at one call per 3 ms with a constant reading.
    fn run(f: &mut BatteryFilter, now: &mut u32, ms: u32, raw: u16, tx: bool) {
        for _ in 0..ms / 3 {
            *now = now.wrapping_add(30);
            f.update(*now, raw, tx, &CAL);
        }
    }

    #[test]
    fn first_reading_is_the_start() {
        let (f, _) = started(2537);
        assert_eq!(f.raw12(), 2537);
        assert_eq!(f.bars(), 4);
        let (f, _) = started(0);
        assert_eq!((f.raw12(), f.bars()), (0, 0));
        let (f, _) = started(4095);
        assert_eq!((f.raw12(), f.bars()), (4095, 4));
    }

    #[test]
    fn receiving_moves_over_seconds() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 1000, 2400, false);
        assert!(f.raw12() > 2600 - 60, "after 1 s {}", f.raw12()); // under 30% of the 200 LSB step
        run(&mut f, &mut now, 2200, 2400, false);
        let at3 = f.raw12();
        assert!(at3 < 2600 - 100 && at3 > 2600 - 140, "after 3.2 s {}", at3); // about 63%
        run(&mut f, &mut now, 7000, 2400, false);
        assert!(f.raw12() <= 2420, "after 10 s {}", f.raw12()); // over 90%
        run(&mut f, &mut now, 20000, 2400, false);
        assert!(f.raw12() <= 2404 && f.raw12() >= 2396);
    }

    #[test]
    fn sag_shows_within_a_second_of_keying() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 500, 2600, false);
        run(&mut f, &mut now, 1000, 2400, true);
        assert!(f.raw12() <= 2410, "{}", f.raw12()); // 95% of the 200 LSB drop
        run(&mut f, &mut now, 300, 2400, true);
        assert!(f.raw12() <= 2403);
    }

    #[test]
    fn unkey_blends_back_without_a_jump() {
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 2000, 2400, true);
        let sag = f.raw12();
        let mut prev = sag;
        let mut biggest = 0;
        for _ in 0..(10_000 / 3) {
            now = now.wrapping_add(30);
            f.update(now, 2600, false, &CAL);
            assert!(f.raw12() >= prev, "no dip on the way back");
            biggest = biggest.max(f.raw12() - prev);
            prev = f.raw12();
        }
        assert!(biggest <= 16, "biggest single move {}", biggest); // a tenth of the 200 LSB gap, not a snap
        assert!(f.raw12() >= 2600 - HYST_RX as u16 - 1);
        // after 2 s it is most of the way back
        let (mut f, mut now) = started(2600);
        run(&mut f, &mut now, 2000, 2400, true);
        run(&mut f, &mut now, 2000, 2600, false);
        assert!(f.raw12() >= 2600 - 25, "{}", f.raw12());
    }

    #[test]
    fn noise_never_changes_the_value_or_the_bars() {
        let mut seed: u32 = 12345;
        for centre in [2537u16, 2560, 2300, 3000, 2400] {
            let (mut f, mut now) = started(centre);
            let (v0, b0) = (f.raw12(), f.bars());
            for i in 0..20_000u32 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 16) % 7; // 0..=6, i.e. +-3 LSB
                let raw = centre + noise as u16 - 3;
                now = now.wrapping_add(30);
                f.update(now, if i % 2 == 0 { raw } else { centre + (i % 3) as u16 - 1 }, false, &CAL);
                assert_eq!((f.raw12(), f.bars()), (v0, b0));
            }
        }
    }

    #[test]
    fn bars_do_not_toggle_at_a_threshold() {
        // threshold cal[3] = 148 (8-bit): 2400 is raw8 150, 2 bars; the average of 2360 and 2400 is raw8 148
        let (mut f, mut now) = started(2400);
        assert_eq!(f.bars(), 2);
        let mut changes = 0;
        let mut last = f.bars();
        for i in 0..2000 {
            now = now.wrapping_add(30);
            f.update(now, if i % 2 == 0 { 2360 } else { 2400 }, false, &CAL); // average sits at 2380, raw8 148
            if f.bars() != last {
                changes += 1;
                last = f.bars();
            }
        }
        assert!(changes <= 1, "{} bar changes", changes);
    }

    #[test]
    fn extremes_do_not_overflow() {
        let (mut f, mut now) = started(4095);
        run(&mut f, &mut now, 30_000, 0, false);
        assert!(f.raw12() <= 4);
        run(&mut f, &mut now, 30_000, 4095, true);
        assert!(f.raw12() >= 4090);
        run(&mut f, &mut now, 3_000, 65535, false); // out of range readings are clamped
        assert!(f.raw12() <= 4095);
        let (mut f, mut now) = started(0);
        run(&mut f, &mut now, 30_000, 4095, false);
        assert!(f.raw12() >= 4090 && f.bars() == 4);
    }

    #[test]
    fn the_clock_wrapping_and_a_stalled_loop_are_harmless() {
        let mut f = BatteryFilter::new();
        f.init(2500, u32::MAX - 100, &CAL);
        let mut now = u32::MAX - 100;
        run(&mut f, &mut now, 3000, 2500, false);
        assert_eq!(f.raw12(), 2500);
        // a stall of 10 s counts as 1 s of steps, not an unbounded loop
        now = now.wrapping_add(100_000);
        f.update(now, 2300, false, &CAL);
        assert!(f.raw12() > 2300 + 100);
    }
}
