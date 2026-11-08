use crate::fmtlite::Sink;
use super::list::{draw_list, Cache, ListSource};
use crate::app;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::mono_font::{MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;

struct LauncherSource<'a, 'b>(&'a mut app::App<'b>);

impl<'a, 'b> ListSource for LauncherSource<'a, 'b> {
    fn row_count(&mut self) -> usize {
        self.0.launcher_item_count()
    }

    fn read_flash(&mut self, addr: u32, buf: &mut [u8]) {
        self.0.storage_mut().read_raw(addr, buf);
    }

    fn label(&mut self, index: usize, w: &mut dyn crate::fmtlite::Sink) {
        self.0.launcher_label_at(index, w);
    }

    fn value(&mut self, index: usize, w: &mut dyn crate::fmtlite::Sink) -> bool {
        if self.0.launcher_available_at(index) {
            false
        } else {
            w.put_str("----");
            true
        }
    }
}

pub fn draw_app_menu<D>(lcd: &mut D, app: &mut app::App, cache: &mut Cache)
where
    D: DrawTarget<Color = Rgb565>,
{
    let index = app.launcher_index();
    let mut source = LauncherSource(app);
    draw_list(lcd, "MENU", &mut source, index, false, cache);

    let err = crate::app::overlay::take_load_err();
    if err != 0 {
        let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
            .font(&FONT_5X8)
            .text_color(Rgb565::RED)
            .build();
        let mut buf: crate::ui::TextBuf<16> = crate::ui::TextBuf::new();
        buf.put_str("LDERR ");
        buf.put_u32(err as u32);
        Text::new(
            buf.as_str(),
            Point::new(2, crate::drivers::st7735::HEIGHT as i32 - 9),
            style,
        )
        .draw(lcd)
        .ok();
    }
}
