//! Draws a personal boot screen (callsign, name, radio, firmware build) as a 160x128 boot logo.
//!
//!   sim bootcard ENV_FILE OUT_PREFIX FW_VERSION BUILD_DATE     (scripts/dev.sh personalize)
//!
//! Writes OUT_PREFIX.rgb565 (40,960 bytes, little-endian RGB565: the radio's boot-logo format) and OUT_PREFIX.png.
//! ENV_FILE is a `KEY=VALUE` file (see .env.sample): OWNER_CALLSIGN, OWNER_NAME, RADIO_MODEL, RADIO_BOARD.
//! It uses the same fonts and colours as the firmware's lists, so the boot screen matches the rest of the UI.
//! The core has no version string of its own (that keeps its image reproducible), so the build goes here.
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10, FONT_9X18};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use std::collections::HashMap;

const W: i32 = 160;
const H: i32 = 128;
const TITLE_BG: Rgb565 = Rgb565::new(27, 54, 27);
const ACCENT: Rgb565 = Rgb565::new(4, 24, 26);
const STRIP_BG: Rgb565 = Rgb565::new(4, 9, 4);
const DIM: Rgb565 = Rgb565::new(20, 40, 20);

struct Canvas(Vec<u16>);

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Canvas {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && p.x < W && p.y < H {
                self.0[(p.y * W + p.x) as usize] = RawU16::from(c).into_inner();
            }
        }
        Ok(())
    }
}

/// `KEY=VALUE` lines; `#` comments; optional single or double quotes around the value. Nothing is executed.
pub fn parse_env(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(v);
            out.insert(k.trim().to_string(), v.to_string());
        }
    }
    out
}

fn fill(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(c)
        .ok();
}

fn text(c: &mut Canvas, s: &str, font: &MonoFont<'_>, x: i32, top: i32, color: Rgb565) {
    Text::new(s, Point::new(x, top + font.baseline as i32), MonoTextStyle::new(font, color)).draw(c).ok();
}

fn width(s: &str, font: &MonoFont<'_>) -> i32 {
    s.chars().count() as i32 * font.character_size.width as i32
}

/// Text drawn `scale` times larger, by drawing it small off-screen and copying each pixel as a block.
fn text_scaled(c: &mut Canvas, s: &str, font: &MonoFont<'_>, scale: i32, x: i32, top: i32, color: Rgb565) {
    let mut small = Canvas(vec![0; (W * H) as usize]);
    text(&mut small, s, font, 0, 0, Rgb565::WHITE);
    let (tw, th) = (width(s, font).min(W), font.character_size.height as i32);
    for yy in 0..th {
        for xx in 0..tw {
            if small.0[(yy * W + xx) as usize] != 0 {
                fill(c, x + xx * scale, top + yy * scale, scale, scale, color);
            }
        }
    }
}

fn ascii(s: &str, max: usize) -> String {
    s.chars().filter(|ch| (' '..='~').contains(ch)).take(max).collect()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let [env_file, out_prefix, fw_version, build_date] = args else {
        return Err("usage: sim bootcard ENV_FILE OUT_PREFIX FW_VERSION BUILD_DATE".into());
    };
    let env = parse_env(&std::fs::read_to_string(env_file).map_err(|e| format!("{env_file}: {e}"))?);
    let get = |k: &str, max: usize| ascii(env.get(k).map(String::as_str).unwrap_or(""), max);
    let callsign = get("OWNER_CALLSIGN", 10).to_uppercase();
    if callsign.is_empty() {
        return Err(format!("{env_file}: OWNER_CALLSIGN is not set (copy .env.sample to .env and fill it in)"));
    }
    let name = get("OWNER_NAME", 26);
    let model = get("RADIO_MODEL", 12);
    let board = get("RADIO_BOARD", 14);

    let mut c = Canvas(vec![0; (W * H) as usize]);
    // title bar: the radio and its board
    fill(&mut c, 0, 0, W, 14, TITLE_BG);
    text(&mut c, &model, &FONT_6X10, 4, 2, Rgb565::BLACK);
    text(&mut c, &board, &FONT_6X10, W - 4 - width(&board, &FONT_6X10), 2, Rgb565::BLACK);
    // callsign, as large as fits
    let scale = if width(&callsign, &FONT_10X20) * 2 <= W - 8 { 2 } else { 1 };
    let cw = width(&callsign, &FONT_10X20) * scale;
    let ctop = if scale == 2 { 24 } else { 34 };
    text_scaled(&mut c, &callsign, &FONT_10X20, scale, (W - cw) / 2, ctop, Rgb565::WHITE);
    fill(&mut c, 20, 70, W - 40, 2, ACCENT);
    // name, in the largest font that fits
    let nfont = if width(&name, &FONT_9X18) <= W - 8 { &FONT_9X18 } else { &FONT_6X10 };
    text(&mut c, &name, nfont, (W - width(&name, nfont)) / 2, 80, Rgb565::new(27, 54, 27));
    // bottom strip: which build this is
    fill(&mut c, 0, H - 16, W, 16, STRIP_BG);
    let fw = format!("FW {}", ascii(fw_version, 14));
    text(&mut c, &fw, &FONT_6X10, 4, H - 13, DIM);
    let date = ascii(build_date, 10);
    text(&mut c, &date, &FONT_6X10, W - 4 - width(&date, &FONT_6X10), H - 13, DIM);

    let mut raw = Vec::with_capacity(c.0.len() * 2);
    let mut rgb = Vec::with_capacity(c.0.len() * 3);
    for &p in &c.0 {
        raw.extend_from_slice(&p.to_le_bytes());
        let (r, g, b) = ((p >> 11) & 31, (p >> 5) & 63, p & 31);
        rgb.extend_from_slice(&[(r << 3 | r >> 2) as u8, (g << 2 | g >> 4) as u8, (b << 3 | b >> 2) as u8]);
    }
    std::fs::write(format!("{out_prefix}.rgb565"), &raw).map_err(|e| e.to_string())?;
    std::fs::write(format!("{out_prefix}.png"), crate::scenario::png_encode(W as usize, H as usize, &rgb)).map_err(|e| e.to_string())?;
    println!("wrote {out_prefix}.rgb565 ({} bytes) and {out_prefix}.png: {callsign} / {name} / {model} {board} / {fw} {date}", raw.len());
    Ok(())
}
