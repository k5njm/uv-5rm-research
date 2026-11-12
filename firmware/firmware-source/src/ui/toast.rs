//! A notice box in the middle of the screen, in the style of the standby screen's TX PROHIBITED box.
//! Only `embedded_graphics`: shared with the host simulator, which draws app toasts with this same code.
use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use super::{SCREEN_H, SCREEN_W};

/// Confirmation colour (saved, done).
pub(crate) const OK: Rgb565 = Rgb565::new(4, 52, 10);

pub(crate) fn draw_toast<D>(lcd: &mut D, label: &str, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let char_w = FONT_6X10.character_size.width as i32;
    let max_chars = ((SCREEN_W - 20) / char_w) as usize;
    let label = &label[..label.len().min(max_chars)];
    let width = label.len() as i32 * char_w + 16;
    let x = (SCREEN_W - width) / 2;
    let y = SCREEN_H / 2 - 11;
    Rectangle::new(Point::new(x, y), Size::new(width as u32, 22))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(lcd)
        .ok();
    Rectangle::new(Point::new(x, y), Size::new(width as u32, 22))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(lcd)
        .ok();
    Text::new(
        label,
        Point::new(x + 8, y + FONT_6X10.baseline as i32 + 6),
        MonoTextStyle::new(&FONT_6X10, color),
    )
    .draw(lcd)
    .ok();
}
