use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use super::{TextBuf, SCREEN_H, SCREEN_W};

const BG: Rgb565 = Rgb565::BLACK;
const FG: Rgb565 = Rgb565::WHITE;
// Colours carry meaning: a light title bar, blue for "you are here", amber for "you are changing this",
// grey for values that are only being shown.
const TITLE_BG: Rgb565 = Rgb565::new(27, 54, 27);
const TITLE_FG: Rgb565 = Rgb565::BLACK;
const SELECT_BG: Rgb565 = Rgb565::new(4, 24, 26);
const EDIT_BG: Rgb565 = Rgb565::new(31, 42, 0);
const EDIT_FG: Rgb565 = Rgb565::BLACK;
const VALUE_DIM: Rgb565 = Rgb565::new(18, 36, 18);
const HINT_BG: Rgb565 = Rgb565::new(4, 9, 4);
const HINT_FG: Rgb565 = Rgb565::new(20, 40, 20);

const ROW_HEIGHT: i32 = FONT_6X10.character_size.height as i32 + 4;
const TITLE_HEIGHT: i32 = ROW_HEIGHT;
const LIST_TOP: i32 = TITLE_HEIGHT;
/// The bottom strip says what the keys do on this screen ("MENU open  EXIT back"); every list keeps it, so lists
/// always show the same number of rows. Apps size their row buffers to `VISIBLE_ROWS` (7).
const HINT_HEIGHT: i32 = ROW_HEIGHT + 2;
const VISIBLE_ROWS: usize = ((SCREEN_H - LIST_TOP - HINT_HEIGHT) / ROW_HEIGHT) as usize;
const HINT_TOP: i32 = SCREEN_H - HINT_HEIGHT;
const RIGHT_MARGIN: i32 = 4;
const SCROLL_W: i32 = 2;

const ARROW_GAP: i32 = 6;

const TITLE_MAX: usize = 26; // a full row of the 6-pixel font
const VALUE_MAX: usize = 18;

pub(crate) trait ListSource {
    fn row_count(&mut self) -> usize;
    /// Raw flash read, for the menu backdrop.
    fn read_flash(&mut self, addr: u32, buf: &mut [u8]);
    fn label(&mut self, index: usize, w: &mut dyn crate::fmtlite::Sink);
    fn value(&mut self, _index: usize, _w: &mut dyn crate::fmtlite::Sink) -> bool {
        false
    }
    fn cursor(&mut self, _index: usize) -> Option<usize> {
        None
    }
    /// The row is waiting for a confirming key (delete, reset): it gets the "changing" colour.
    fn armed(&mut self, _index: usize) -> bool {
        false
    }
    /// What the keys do here, for the strip at the bottom of the screen (at most a row of text).
    fn hint(&mut self, _w: &mut dyn crate::fmtlite::Sink) {}
}

pub(crate) struct Cache {
    /// title and hint strip together
    title: Option<TitleKey>,
    selected: Option<usize>,
    total: Option<usize>,
    scroll_top: Option<usize>,
    sel: Option<SelRender>,
}

/// What a piece of text was the last time it was drawn: its length and a hash, not the text. The cache lives in the
/// firmware's RAM for good, and that RAM sits below the app arena, whose address is fixed (`abi::ARENA_ADDR`):
/// keeping three strings here pushed the arena up and hung the radio on every app (AUDIT 31).
#[derive(Clone, Copy, PartialEq)]
struct TitleKey {
    len: u8,
    hash: u32,
}

impl TitleKey {
    fn of(text: &str) -> Self {
        Self::of2(text, "")
    }

    /// One key for two pieces of text (the title and the hint strip share one, to save RAM).
    fn of2(a: &str, b: &str) -> Self {
        let mut hash: u32 = 0x811C_9DC5; // FNV-1a
        for &byte in a.as_bytes().iter().chain(b.as_bytes()) {
            hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
        }
        TitleKey { len: (a.len() + b.len()) as u8, hash }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct SelRender {
    show_arrows: bool,
    value: TitleKey,
    cursor: Option<usize>,
}

fn render_sel(
    source: &mut dyn ListSource,
    selected: usize,
    show_arrows: bool,
) -> SelRender {
    let mut value: TextBuf<VALUE_MAX> = TextBuf::new();
    let has_value = source.value(selected, &mut value);
    let cursor = if has_value {
        source.cursor(selected)
    } else {
        None
    };
    SelRender {
        show_arrows,
        value: TitleKey::of(value.as_str()),
        cursor,
    }
}

impl Cache {
    pub fn new() -> Self {
        Cache {
            title: None,
            selected: None,
            total: None,
            scroll_top: None,
            sel: None,
        }
    }

    fn matches_title(&self, title: &str, hint: &str) -> bool {
        self.title == Some(TitleKey::of2(title, hint))
    }
}

pub(crate) fn draw_list<D>(
    lcd: &mut D,
    title: &str,
    source: &mut dyn ListSource,
    selected: usize,
    show_arrows: bool,
    cache: &mut Cache,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let total = source.row_count();
    let top = scroll_top(total, selected);

    let mut hint: TextBuf<TITLE_MAX> = TextBuf::new();
    source.hint(&mut hint);
    // a new title or a new hint repaints the whole list
    let title_changed = !cache.matches_title(title, hint.as_str());
    let first = cache.selected.is_none() || title_changed;
    if first {
        draw_hint(lcd, hint.as_str());
    }

    let mut rows_drawn = true;
    if first {
        draw_full(lcd, title, source, selected, show_arrows, total, top);
    } else {
        let prev_selected = cache.selected.unwrap();
        let prev_top = cache.scroll_top.unwrap();
        let prev_total = cache.total.unwrap();

        if top != prev_top || total != prev_total {
            for slot in 0..VISIBLE_ROWS {
                draw_row(
                    lcd,
                    source,
                    top + slot,
                    selected,
                    show_arrows,
                    total,
                    slot,
                );
            }
        } else if selected != prev_selected {
            draw_row(
                lcd,
                source,
                prev_selected,
                selected,
                show_arrows,
                total,
                slot_of(prev_selected, top),
            );
            draw_row(
                lcd,
                source,
                selected,
                selected,
                show_arrows,
                total,
                slot_of(selected, top),
            );
        } else {
            if cache.sel != Some(render_sel(source, selected, show_arrows)) {
                draw_row(
                    lcd,
                    source,
                    selected,
                    selected,
                    show_arrows,
                    total,
                    slot_of(selected, top),
                );
            } else {
                rows_drawn = false;
            }
        }
    }
    if rows_drawn {
        draw_scroll_mark(lcd, total, top);
    }

    cache.title = Some(TitleKey::of2(title, hint.as_str()));
    cache.selected = Some(selected);
    cache.total = Some(total);
    cache.scroll_top = Some(top);
    cache.sel = Some(render_sel(source, selected, show_arrows));
}

fn slot_of(index: usize, top: usize) -> usize {
    index - top
}

/// Where a list of `total` rows scrolled to `top` puts its position mark: (offset from the top of the rows,
/// height), in pixels. None when every row fits on the screen.
fn scroll_mark(total: usize, top: usize) -> Option<(i32, i32)> {
    if total <= VISIBLE_ROWS {
        return None;
    }
    let track = VISIBLE_ROWS as i32 * ROW_HEIGHT;
    let height = (track * VISIBLE_ROWS as i32 / total as i32).max(6);
    let steps = (total - VISIBLE_ROWS) as i32;
    Some(((track - height) * top.min(total - VISIBLE_ROWS) as i32 / steps, height))
}

/// A list longer than the screen shows where it is: a mark on the right edge, sized to the part on show. Rows paint
/// the full width, so this is drawn after them.
fn draw_scroll_mark<D>(lcd: &mut D, total: usize, top: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    if let Some((offset, height)) = scroll_mark(total, top) {
        Rectangle::new(
            Point::new(SCREEN_W - SCROLL_W, LIST_TOP + offset),
            Size::new(SCROLL_W as u32, height as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(VALUE_DIM))
        .draw(lcd)
        .ok();
    }
}

fn draw_full<D>(
    lcd: &mut D,
    title: &str,
    source: &mut dyn ListSource,
    selected: usize,
    show_arrows: bool,
    total: usize,
    top: usize,
) where
    D: DrawTarget<Color = Rgb565>,
{
    draw_title(lcd, title);

    for slot in 0..VISIBLE_ROWS {
        draw_row(lcd, source, top + slot, selected, show_arrows, total, slot);
    }
    let bottom = LIST_TOP + VISIBLE_ROWS as i32 * ROW_HEIGHT;
    if HINT_TOP > bottom {
        clear_rows(lcd, bottom, HINT_TOP - bottom);
    }
}

fn draw_hint<D>(lcd: &mut D, hint: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(
        Point::new(0, HINT_TOP),
        Size::new(SCREEN_W as u32, HINT_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(HINT_BG))
    .draw(lcd)
    .ok();
    Text::new(
        hint,
        Point::new(4, HINT_TOP + FONT_6X10.baseline as i32 + 3),
        MonoTextStyle::new(&FONT_6X10, HINT_FG),
    )
    .draw(lcd)
    .ok();
}

/// Plain background behind rows. (Lists used to paint a dimmed copy of the boot logo here; under text it only
/// made the rows harder to read.)
fn clear_rows<D>(lcd: &mut D, top: i32, height: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(0, top), Size::new(SCREEN_W as u32, height as u32))
        .into_styled(PrimitiveStyle::with_fill(BG))
        .draw(lcd)
        .ok();
}

fn draw_title<D>(lcd: &mut D, title: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(
        Point::new(0, 0),
        Size::new(SCREEN_W as u32, TITLE_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(TITLE_BG))
    .draw(lcd)
    .ok();
    draw_centered(lcd, title, FONT_6X10.baseline as i32 + 2);
}

fn draw_row<D>(
    lcd: &mut D,
    source: &mut dyn ListSource,
    index: usize,
    selected: usize,
    show_arrows: bool,
    total: usize,
    slot: usize,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let row_top = LIST_TOP + slot as i32 * ROW_HEIGHT;
    let is_selected = index < total && index == selected;
    if !is_selected {
        clear_rows(lcd, row_top, ROW_HEIGHT);
    }

    if index >= total {
        return; // blank row below the end of a shorter list
    }

    // "You are changing this": a value with arrows, or a typed field (it has a text cursor).
    let editing = is_selected && (show_arrows || source.cursor(index).is_some() || source.armed(index));
    if is_selected {
        Rectangle::new(
            Point::new(0, row_top),
            Size::new(SCREEN_W as u32, ROW_HEIGHT as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(if editing { EDIT_BG } else { SELECT_BG }))
        .draw(lcd)
        .ok();
    }
    let fg = if editing { EDIT_FG } else { FG };
    let value_fg = if is_selected { fg } else { VALUE_DIM };
    let baseline = row_top + FONT_6X10.baseline as i32 + 2;

    let mut label: TextBuf<20> = TextBuf::new();
    source.label(index, &mut label);
    let label_is_empty = label.as_str().is_empty();

    if !label_is_empty {
        Text::new(
            label.as_str(),
            Point::new(4, baseline),
            MonoTextStyle::new(&FONT_6X10, fg),
        )
        .draw(lcd)
        .ok();
    }

    let mut value: TextBuf<18> = TextBuf::new();
    if source.value(index, &mut value) {
        if let Some(cursor) = source.cursor(index) {
            if label_is_empty {
                draw_value_with_cursor_left(
                    lcd,
                    value.as_str(),
                    cursor,
                    row_top,
                    baseline,
                    fg,
                );
            } else {
                draw_value_with_cursor(
                    lcd,
                    value.as_str(),
                    cursor,
                    row_top,
                    baseline,
                    fg,
                );
            }
        } else if is_selected && show_arrows {
            draw_editing_value(lcd, value.as_str(), row_top, fg);
        } else if label_is_empty {
            draw_value_left(lcd, value.as_str(), baseline, fg);
        } else {
            draw_value(lcd, value.as_str(), baseline, value_fg);
        }
    }
}

fn scroll_top(total: usize, selected: usize) -> usize {
    if total <= VISIBLE_ROWS {
        return 0;
    }
    let half = VISIBLE_ROWS / 2;
    selected.saturating_sub(half).min(total - VISIBLE_ROWS)
}

fn draw_centered<D>(lcd: &mut D, text: &str, baseline_y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let width =
        text.chars().count() as i32 * FONT_6X10.character_size.width as i32;
    let x = (SCREEN_W - width) / 2;
    Text::new(
        text,
        Point::new(x, baseline_y),
        MonoTextStyle::new(&FONT_6X10, TITLE_FG),
    )
    .draw(lcd)
    .ok();
}

fn draw_value<D>(lcd: &mut D, text: &str, baseline_y: i32, fg: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let width =
        text.chars().count() as i32 * FONT_6X10.character_size.width as i32;
    let x = SCREEN_W - RIGHT_MARGIN - width;
    Text::new(
        text,
        Point::new(x, baseline_y),
        MonoTextStyle::new(&FONT_6X10, fg),
    )
    .draw(lcd)
    .ok();
}

fn draw_value_left<D>(lcd: &mut D, text: &str, baseline_y: i32, fg: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    Text::new(
        text,
        Point::new(4, baseline_y),
        MonoTextStyle::new(&FONT_6X10, fg),
    )
    .draw(lcd)
    .ok();
}

fn draw_value_with_cursor<D>(
    lcd: &mut D,
    text: &str,
    cursor_index: usize,
    row_top: i32,
    baseline_y: i32,
    fg: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    // A cursor after the last character needs a cell of its own, or it hangs off the edge of the screen.
    let char_w = FONT_6X10.character_size.width as i32;
    let cells = text.chars().count().max(cursor_index + 1) as i32;
    let x0 = SCREEN_W - RIGHT_MARGIN - cells * char_w;
    Text::new(text, Point::new(x0, baseline_y), MonoTextStyle::new(&FONT_6X10, fg))
        .draw(lcd)
        .ok();

    draw_cursor_box(lcd, text, cursor_index, x0, row_top, baseline_y, fg);
}

fn draw_value_with_cursor_left<D>(
    lcd: &mut D,
    text: &str,
    cursor_index: usize,
    row_top: i32,
    baseline_y: i32,
    fg: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    draw_value_left(lcd, text, baseline_y, fg);
    draw_cursor_box(lcd, text, cursor_index, 4, row_top, baseline_y, fg);
}

fn draw_cursor_box<D>(
    lcd: &mut D,
    text: &str,
    cursor_index: usize,
    x0: i32,
    row_top: i32,
    baseline_y: i32,
    fg: Rgb565,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let char_w = FONT_6X10.character_size.width as i32;

    let ch = text.chars().nth(cursor_index).unwrap_or(' ');
    let cursor_x = x0 + cursor_index as i32 * char_w;
    let inverted = if fg == FG { BG } else { FG };
    Rectangle::new(
        Point::new(cursor_x, row_top),
        Size::new(char_w as u32, ROW_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(fg))
    .draw(lcd)
    .ok();

    let mut ch_buf = [0u8; 4];
    Text::new(
        ch.encode_utf8(&mut ch_buf),
        Point::new(cursor_x, baseline_y),
        MonoTextStyle::new(&FONT_6X10, inverted),
    )
    .draw(lcd)
    .ok();
}

/// `'^'`/`'v'` glyphs stand in for up/down arrow triangles
fn draw_editing_value<D>(lcd: &mut D, text: &str, row_top: i32, fg: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let char_w = FONT_6X10.character_size.width as i32;
    let value_width = text.chars().count() as i32 * char_w;
    let total_width = char_w + ARROW_GAP + value_width + ARROW_GAP + char_w;
    let start_x = SCREEN_W - RIGHT_MARGIN - total_width;
    let baseline = row_top + FONT_6X10.baseline as i32 + 2;
    let style = MonoTextStyle::new(&FONT_6X10, fg);

    Text::new("^", Point::new(start_x, baseline), style)
        .draw(lcd)
        .ok();

    let value_x = start_x + char_w + ARROW_GAP;
    Text::new(text, Point::new(value_x, baseline), style)
        .draw(lcd)
        .ok();

    let down_x = value_x + value_width + ARROW_GAP;
    Text::new("v", Point::new(down_x, baseline), style)
        .draw(lcd)
        .ok();
}
