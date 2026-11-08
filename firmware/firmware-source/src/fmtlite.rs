//! Text output without `core::fmt`.
//!
//! The core's screens only ever need ASCII, numbers in decimal/hex/octal with optional zero or space padding, and
//! string pieces. `core::fmt` (Formatter, padding, Display for integers, and the `dyn Write` vtable that drags it all
//! in) cost about 2.5 KB of a 60 KB flash for that. This costs a few hundred bytes.
//!
//! Output is byte-identical to `write!` for every specifier the firmware used (`{}`, `{:02}`, `{:03}`, `{:04}`,
//! `{:05}`, `{:3}`, `{:04X}`, `{:03o}`); the host tests below check that exhaustively against `format!`. They run
//! through the simulator crate, which includes this file: `scripts/dev.sh unit`.

/// Where text goes. Implement `put_str`; the number helpers come for free and work through `&mut dyn Sink`.
pub trait Sink {
    fn put_str(&mut self, s: &str);

    fn put_char(&mut self, c: char) {
        let mut b = [0u8; 4];
        self.put_str(c.encode_utf8(&mut b));
    }

    /// `{}`
    fn put_u32(&mut self, v: u32) {
        self.put_radix(v, 10, 0, b'0', false);
    }

    /// `{}` of a signed number
    fn put_i32(&mut self, v: i32) {
        if v < 0 {
            self.put_char('-');
        }
        self.put_u32(v.unsigned_abs());
    }

    /// `{:0W}` (`pad = b'0'`) or `{:W}` (`pad = b' '`): at least `width` characters, never truncated.
    fn put_u32_w(&mut self, v: u32, width: usize, pad: u8) {
        self.put_radix(v, 10, width, pad, false);
    }

    /// `{:04X}`
    fn put_hex4(&mut self, v: u32) {
        self.put_radix(v, 16, 4, b'0', true);
    }

    /// `{:03o}`
    fn put_oct3(&mut self, v: u32) {
        self.put_radix(v, 8, 3, b'0', false);
    }

    fn put_radix(&mut self, mut v: u32, radix: u32, width: usize, pad: u8, upper: bool) {
        let mut tmp = [0u8; 32];
        let mut i = tmp.len();
        loop {
            i -= 1;
            let d = (v % radix) as u8;
            tmp[i] = if d < 10 { b'0' + d } else { (if upper { b'A' } else { b'a' }) + d - 10 };
            v /= radix;
            if v == 0 {
                break;
            }
        }
        for _ in (tmp.len() - i)..width {
            self.put_char(pad as char);
        }
        // tmp[i..] holds only ASCII digits and letters
        self.put_str(unsafe { core::str::from_utf8_unchecked(&tmp[i..]) });
    }
}

/// A fixed-capacity ASCII string on the stack. Output past `N` bytes is silently dropped (as before); non-ASCII
/// bytes become `?`, which is what makes `as_str` sound without a UTF-8 check.
#[derive(Clone, Copy, PartialEq)]
pub struct TextBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> TextBuf<N> {
    pub fn new() -> Self {
        TextBuf { buf: [0; N], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        // only ASCII is ever stored (see `put_str`)
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }
}

impl<const N: usize> Sink for TextBuf<N> {
    fn put_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len == N {
                break;
            }
            self.buf[self.len] = if b.is_ascii() { b } else { b'?' };
            self.len += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s<const N: usize>(f: impl FnOnce(&mut TextBuf<N>)) -> String {
        let mut b: TextBuf<N> = TextBuf::new();
        f(&mut b);
        b.as_str().to_string()
    }

    /// A spread of values that hits every digit count, powers of ten, boundaries and pseudo-random ones.
    fn samples() -> Vec<u32> {
        let mut v: Vec<u32> = (0..=2000).collect();
        let mut p = 1u32;
        while p < u32::MAX / 10 {
            v.extend([p - 1, p, p + 1]);
            p *= 10;
        }
        v.extend([u32::MAX - 1, u32::MAX, 0x7FFF_FFFF, 0x8000_0000]);
        let mut x = 12345u32;
        for _ in 0..20_000 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            v.push(x);
            v.push(x >> (x % 28));
        }
        v
    }

    #[test]
    fn plain_decimal_matches_std() {
        for n in samples() {
            assert_eq!(s::<12>(|b| b.put_u32(n)), format!("{n}"), "{n}");
        }
    }

    #[test]
    fn zero_padded_widths_used_by_the_firmware_match_std() {
        for n in samples() {
            assert_eq!(s::<12>(|b| b.put_u32_w(n, 2, b'0')), format!("{n:02}"), "{n} w2");
            assert_eq!(s::<12>(|b| b.put_u32_w(n, 3, b'0')), format!("{n:03}"), "{n} w3");
            assert_eq!(s::<12>(|b| b.put_u32_w(n, 4, b'0')), format!("{n:04}"), "{n} w4");
            assert_eq!(s::<12>(|b| b.put_u32_w(n, 5, b'0')), format!("{n:05}"), "{n} w5");
        }
    }

    #[test]
    fn space_padded_width_3_matches_std() {
        for n in samples() {
            assert_eq!(s::<12>(|b| b.put_u32_w(n, 3, b' ')), format!("{n:3}"), "{n}");
        }
    }

    #[test]
    fn hex4_matches_std_for_every_u16_and_beyond() {
        for n in 0..=0xFFFFu32 {
            assert_eq!(s::<8>(|b| b.put_hex4(n)), format!("{n:04X}"), "{n}");
        }
        for n in [0x1_0000, 0xABCDE, u32::MAX] {
            assert_eq!(s::<12>(|b| b.put_hex4(n)), format!("{n:04X}"));
        }
    }

    #[test]
    fn oct3_matches_std() {
        for n in 0..=0o7777u32 {
            assert_eq!(s::<8>(|b| b.put_oct3(n)), format!("{n:03o}"), "{n}");
        }
        assert_eq!(s::<16>(|b| b.put_oct3(u32::MAX)), format!("{:03o}", u32::MAX));
    }

    #[test]
    fn signed_matches_std_including_the_extremes() {
        for n in samples() {
            let i = n as i32;
            assert_eq!(s::<14>(|b| b.put_i32(i)), format!("{i}"), "{i}");
        }
        for i in [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX, -100, -99, -10, -9] {
            assert_eq!(s::<14>(|b| b.put_i32(i)), format!("{i}"), "{i}");
        }
    }

    #[test]
    fn composite_strings_match_the_original_write_calls() {
        for (hz, cv, secs) in [(146_520_000u32, 760u32, 75u32), (462_562_500, 405, 0), (16_000_000, 99, 3599), (999_999_999, 1234, 59)] {
            let mhz = hz / 1_000_000;
            let frac = (hz % 1_000_000) / 10;
            assert_eq!(s::<8>(|b| { b.put_u32_w(secs / 60, 2, b'0'); b.put_char(':'); b.put_u32_w(secs % 60, 2, b'0'); }),
                       format!("{:02}:{:02}", secs / 60, secs % 60));
            assert_eq!(s::<8>(|b| { b.put_u32(cv / 100); b.put_char('.'); b.put_u32_w(cv % 100, 2, b'0'); b.put_str("V"); }),
                       format!("{}.{:02}V", cv / 100, cv % 100));
            assert_eq!(s::<12>(|b| { b.put_u32_w(mhz, 3, b' '); b.put_char('.'); b.put_u32_w(frac / 100, 3, b'0'); }),
                       format!("{:3}.{:03}", mhz, frac / 100));
            assert_eq!(s::<16>(|b| { b.put_u32(mhz); b.put_char('.'); b.put_u32_w(frac, 5, b'0'); }),
                       format!("{mhz}.{frac:05}"));
            assert_eq!(s::<12>(|b| { b.put_u32(hz / 1_000_000); b.put_char('.'); b.put_u32_w(hz / 100 % 10_000, 4, b'0'); }),
                       format!("{}.{:04}", hz / 1_000_000, hz / 100 % 10_000));
        }
    }

    #[test]
    fn capacity_is_respected_and_extra_output_is_dropped() {
        assert_eq!(s::<4>(|b| b.put_str("ABCDEFG")), "ABCD");
        assert_eq!(s::<3>(|b| b.put_u32(123456)), "123");
        assert_eq!(s::<0>(|b| b.put_str("x")), "");
    }

    #[test]
    fn non_ascii_becomes_question_marks_so_as_str_stays_valid() {
        assert_eq!(s::<8>(|b| b.put_str("a\u{e9}b")), "a??b");
        assert_eq!(s::<8>(|b| b.put_char('\u{1F4FB}')), "????");
    }

    #[test]
    fn works_through_a_trait_object() {
        let mut b: TextBuf<16> = TextBuf::new();
        let w: &mut dyn Sink = &mut b;
        w.put_str("CH ");
        w.put_u32_w(7, 3, b'0');
        w.put_char(' ');
        w.put_i32(-5);
        assert_eq!(b.as_str(), "CH 007 -5");
    }
}
