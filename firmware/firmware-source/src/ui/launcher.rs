use crate::fmtlite::Sink;
use super::list::{draw_list, Cache, ListSource};
use crate::app;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

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

    fn hint(&mut self, w: &mut dyn crate::fmtlite::Sink) {
        w.put_str("MENU or 1-9 open EXIT back");
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
    draw_list(lcd, "APPS", &mut source, index, false, cache);

    let err = crate::app::overlay::take_load_err();
    if err != 0 {
        // a notice box, clear of the key-hint strip; the number is the loader's error code
        let mut buf: crate::ui::TextBuf<24> = crate::ui::TextBuf::new();
        buf.put_str("APP DID NOT LOAD (");
        buf.put_u32(err as u32);
        buf.put_str(")");
        crate::ui::toast::draw_toast(lcd, buf.as_str(), Rgb565::new(31, 8, 4));
    }
}
