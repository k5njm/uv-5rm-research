//! FSK probe: a hardware bring-up tool for the radio's FSK modem. It answers "does this chip's modem hear anything,
//! and what do its registers say?" before any modem app (APRS, Horus, RS41) is trusted.
//!
//! Receive-only. It never keys the transmitter and never calls `api.tone_on` or `api.set_ptt`. Chip access goes
//! through `api.rf_reg_read` / `api.rf_reg_write`; the core refuses writes outside the modem allow-list
//! (`bf5rh_abi::RF_WRITABLE`), and this app only ever writes registers on that list.
//!
//! UNVERIFIED on hardware: the baud divisor formula (scaled from the 1200 baud value the APRS app uses), the sync
//! words, and the FIFO/threshold semantics are carried over from the other modem apps, not measured. Finding out
//! which of them are right on the YP07's FD6818B is the point of this app.

use bf5rh_abi::{Api, AppEvent, AppResult};

const KIND_SINGLE: u8 = 2;
const KEY_MENU: u8 = 12;
const KEY_EXIT: u8 = 13;
const KEY_UP: u8 = 14;
const KEY_DOWN: u8 = 15;
const KEY_STAR: u8 = 10;

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GRAY: u16 = 0x8410;
const TITLE_BG: u16 = 0x5800;
const GREEN: u16 = 0x07E0;
const YELLOW: u16 = 0xFFE0;

const R_INT: u8 = 0x02;
const R_STATUS: u8 = 0x0C;
const R_IRQ_EN: u8 = 0x3F;
const R_FSK: u8 = 0x58;
const R_CTRL: u8 = 0x59;
const R_SYNC1: u8 = 0x5A;
const R_SYNC2: u8 = 0x5B;
const R_CRC: u8 = 0x5C;
const R_SIZE: u8 = 0x5D;
const R_THRESH: u8 = 0x5E;
const R_FIFO: u8 = 0x5F;
const R_GAIN: u8 = 0x70;
const R_BAUD: u8 = 0x72;

const IRQ_SYNC: u16 = 1 << 1;
const IRQ_RX_DONE: u16 = 1 << 13;
const IRQ_WORD: u16 = 1 << 12;

const CTRL_CLEAR: u16 = (1 << 15) | (1 << 14);
const CTRL_RX: u16 = (1 << 12) | (1 << 10) | (1 << 8);
const CTRL_RX_ON: u16 = 1 << 14;

/// Registers the peek view cycles through. Reading is unrestricted; this is just the interesting set.
const PEEK: [u8; 14] = [
    0x02, 0x0C, 0x2B, 0x30, 0x3F, 0x40, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x72,
];

/// Frequencies in Hz: APRS, 2 m calling, 70 cm calling, Horus 70 cm.
const FREQS: [u32; 4] = [144_390_000, 146_520_000, 446_000_000, 434_714_000];
const BAUDS: [u16; 4] = [450, 1200, 2400, 4800];
const SYNCS: [u16; 3] = [0x7E7E, 0x2DD4, 0xAAAA];

/// 1200 baud is 4646 on the register the APRS app writes; assume it scales linearly (UNVERIFIED).
const BAUD_REG_1200: u32 = 4646;

const SCREEN_W: u16 = 160;
const LINE_H: i16 = 10;

struct State {
    freq: usize,
    baud: usize,
    sync: usize,
    running: bool,
    peek: usize,
    syncs: u16,
    words: u16,
    last: [u16; 4],
    status: u16,
    irq: u16,
    dirty: bool,
}

static mut STATE: State = STATE_DEFAULT;

fn st() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

pub fn entry(api: &Api, ev: AppEvent) -> AppResult {
    match ev {
        AppEvent::Enter => {
            *st() = STATE_DEFAULT;
            tune(api, st());
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => key(api, id, kind),
        AppEvent::Tick { .. } => {
            tick(api, st());
            AppResult::Continue
        }
        AppEvent::Draw => {
            draw(api, st());
            AppResult::Continue
        }
        AppEvent::Leave => {
            modem_off(api);
            AppResult::Continue
        }
    }
}

const STATE_DEFAULT: State = State {
    freq: 0,
    baud: 1,
    sync: 0,
    running: false,
    peek: 0,
    syncs: 0,
    words: 0,
    last: [0; 4],
    status: 0,
    irq: 0,
    dirty: true,
};

/// Listen on the chosen frequency in narrow FM; the modem taps the demodulator.
fn tune(api: &Api, s: &State) {
    (api.set_modulation)(0);
    (api.set_rx_freq)(FREQS[s.freq]);
    (api.set_subaudio_rx)(0);
    (api.set_sql_level)((api.settings_get)(0) as u8);
    (api.enter_rx)();
}

fn w(api: &Api, addr: u8, v: u16) {
    // A refused write means the allow-list and this app disagree; surface it instead of ignoring it.
    if !(api.rf_reg_write)(addr, v) {
        (api.app_fault)(addr as u32);
    }
}

fn modem_on(api: &Api, s: &mut State) {
    w(api, R_GAIN, (1 << 7) | 96);
    w(api, R_BAUD, baud_reg(BAUDS[s.baud]));
    w(api, R_FSK, 0x0301);
    w(api, R_SYNC1, SYNCS[s.sync]);
    w(api, R_SYNC2, SYNCS[s.sync]);
    w(api, R_CRC, 0x5625);
    w(api, R_SIZE, 64u16 << 8);
    w(api, R_THRESH, (1 << 3) | 1);
    w(api, R_IRQ_EN, IRQ_RX_DONE | IRQ_WORD | IRQ_SYNC);
    w(api, R_CTRL, CTRL_RX | CTRL_CLEAR);
    w(api, R_CTRL, CTRL_RX | CTRL_RX_ON);
    s.running = true;
    s.syncs = 0;
    s.words = 0;
    s.last = [0; 4];
}

fn modem_off(api: &Api) {
    w(api, R_IRQ_EN, 0);
    w(api, R_CTRL, CTRL_CLEAR);
    st().running = false;
}

fn baud_reg(baud: u16) -> u16 {
    (baud as u32 * BAUD_REG_1200 / 1200) as u16
}

fn key(api: &Api, id: u8, kind: u8) -> AppResult {
    if kind != KIND_SINGLE {
        return AppResult::Continue;
    }
    let s = st();
    match id {
        KEY_EXIT => {
            modem_off(api);
            return AppResult::Exit;
        }
        KEY_MENU => {
            if s.running {
                modem_off(api);
            } else {
                modem_on(api, s);
            }
        }
        KEY_STAR => {
            s.freq = (s.freq + 1) % FREQS.len();
            let was = s.running;
            modem_off(api);
            tune(api, s);
            if was {
                modem_on(api, s);
            }
        }
        KEY_UP => s.peek = (s.peek + PEEK.len() - 1) % PEEK.len(),
        KEY_DOWN => s.peek = (s.peek + 1) % PEEK.len(),
        1..=4 => {
            s.baud = (id - 1) as usize;
            if s.running {
                w(api, R_BAUD, baud_reg(BAUDS[s.baud]));
            }
        }
        5..=7 => {
            s.sync = (id - 5) as usize;
            if s.running {
                w(api, R_SYNC1, SYNCS[s.sync]);
                w(api, R_SYNC2, SYNCS[s.sync]);
            }
        }
        _ => return AppResult::Continue,
    }
    s.dirty = true;
    AppResult::Continue
}

fn tick(api: &Api, s: &mut State) {
    let status = (api.rf_reg_read)(R_STATUS);
    if status != s.status {
        s.status = status;
        s.dirty = true;
    }
    if !s.running || status & 1 == 0 {
        return;
    }
    // Writing the interrupt register latches the flags; the read returns them.
    w(api, R_INT, 0);
    let irq = (api.rf_reg_read)(R_INT);
    s.irq = irq;
    if irq & IRQ_SYNC != 0 {
        s.syncs = s.syncs.saturating_add(1);
    }
    if irq & (IRQ_WORD | IRQ_RX_DONE) != 0 {
        let word = (api.rf_reg_read)(R_FIFO);
        s.words = s.words.saturating_add(1);
        s.last = [s.last[1], s.last[2], s.last[3], word];
    }
    s.dirty = true;
}

fn draw(api: &Api, s: &mut State) {
    // The RSSI line moves constantly, so it is redrawn every frame; the rest only when something changed.
    let raw = (api.read_rssi)();
    let dbm = raw as i32 - 170;
    let mut b = [0u8; 28];
    let mut n = 0;
    n = put(&mut b, n, b"RSSI ");
    n = put_i(&mut b, n, dbm);
    n = put(&mut b, n, b" dBm  raw ");
    n = put_u(&mut b, n, raw as u32);
    line(api, 3, &b[..n], WHITE);
    if !s.dirty {
        return;
    }
    s.dirty = false;

    (api.fill_rect)(0, 0, SCREEN_W, LINE_H as u16, TITLE_BG);
    text(api, 2, 1, b"FSK PROBE  RX ONLY", WHITE, TITLE_BG);

    n = 0;
    n = put_freq(&mut b, n, FREQS[s.freq]);
    n = put(&mut b, n, b"  ");
    n = put_u(&mut b, n, BAUDS[s.baud] as u32);
    n = put(&mut b, n, b" baud");
    line(api, 1, &b[..n], WHITE);

    line(api, 2, if s.running { b"MODEM ON (rx)" } else { b"MODEM OFF" }, if s.running { GREEN } else { GRAY });

    n = 0;
    n = put(&mut b, n, b"SYNC ");
    n = put_hex(&mut b, n, SYNCS[s.sync]);
    n = put(&mut b, n, b" BAUDREG ");
    n = put_hex(&mut b, n, baud_reg(BAUDS[s.baud]));
    line(api, 4, &b[..n], WHITE);

    n = 0;
    n = put(&mut b, n, b"STAT ");
    n = put_hex(&mut b, n, s.status);
    n = put(&mut b, n, b" IRQ ");
    n = put_hex(&mut b, n, s.irq);
    line(api, 5, &b[..n], WHITE);

    n = 0;
    n = put(&mut b, n, b"SYNCS ");
    n = put_u(&mut b, n, s.syncs as u32);
    n = put(&mut b, n, b" WORDS ");
    n = put_u(&mut b, n, s.words as u32);
    line(api, 6, &b[..n], YELLOW);

    n = 0;
    for (i, wd) in s.last.iter().enumerate() {
        if i > 0 {
            n = put(&mut b, n, b" ");
        }
        n = put_hex(&mut b, n, *wd);
    }
    line(api, 7, &b[..n], WHITE);

    let reg = PEEK[s.peek];
    n = 0;
    n = put(&mut b, n, b"REG ");
    n = put_hex2(&mut b, n, reg);
    n = put(&mut b, n, b" = ");
    n = put_hex(&mut b, n, (api.rf_reg_read)(reg));
    line(api, 8, &b[..n], WHITE);

    line(api, 9, b"1-4 baud  5-7 sync", GRAY);
    line(api, 10, b"MENU on/off  * freq", GRAY);
    line(api, 11, b"UP/DN reg  EXIT leave", GRAY);
}

fn text(api: &Api, x: i16, y: i16, s: &[u8], fg: u16, bg: u16) {
    (api.draw_text)(x, y, s.as_ptr(), s.len() as u16, fg, bg, 0);
}

fn line(api: &Api, row: i16, s: &[u8], fg: u16) {
    let y = row * LINE_H + 2;
    (api.fill_rect)(0, y, 2, LINE_H as u16, BLACK);
    let end = (api.draw_text)(2, y, s.as_ptr(), s.len() as u16, fg, BLACK, 0);
    (api.fill_rect)(end as i16, y, SCREEN_W.saturating_sub(end), LINE_H as u16, BLACK);
}

fn put(b: &mut [u8], n: usize, s: &[u8]) -> usize {
    let k = s.len().min(b.len() - n);
    b[n..n + k].copy_from_slice(&s[..k]);
    n + k
}

fn put_u(b: &mut [u8], n: usize, mut v: u32) -> usize {
    let mut d = [0u8; 10];
    let mut i = d.len();
    loop {
        i -= 1;
        d[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    put(b, n, &d[i..])
}

fn put_i(b: &mut [u8], n: usize, v: i32) -> usize {
    if v < 0 {
        let n = put(b, n, b"-");
        put_u(b, n, v.unsigned_abs())
    } else {
        put_u(b, n, v as u32)
    }
}

fn put_hex(b: &mut [u8], n: usize, v: u16) -> usize {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let d = [H[(v >> 12) as usize & 15], H[(v >> 8) as usize & 15], H[(v >> 4) as usize & 15], H[v as usize & 15]];
    put(b, n, &d)
}

fn put_hex2(b: &mut [u8], n: usize, v: u8) -> usize {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    put(b, n, &[H[(v >> 4) as usize & 15], H[v as usize & 15]])
}

/// `144.390` style (MHz with three decimals).
fn put_freq(b: &mut [u8], n: usize, hz: u32) -> usize {
    let n = put_u(b, n, hz / 1_000_000);
    let n = put(b, n, b".");
    let k = hz / 1_000 % 1_000;
    let n = put(b, n, &[b'0' + (k / 100) as u8, b'0' + (k / 10 % 10) as u8, b'0' + (k % 10) as u8]);
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baud_register_matches_the_known_1200_value_and_scales() {
        assert_eq!(baud_reg(1200), 4646);
        assert_eq!(baud_reg(450), 1742);
        assert_eq!(baud_reg(2400), 9292);
        assert_eq!(baud_reg(4800), 18584);
    }

    #[test]
    fn every_register_the_app_writes_is_on_the_allow_list() {
        for r in [R_INT, R_IRQ_EN, R_FSK, R_CTRL, R_SYNC1, R_SYNC2, R_CRC, R_SIZE, R_THRESH, R_FIFO, R_GAIN, R_BAUD] {
            assert!(bf5rh_abi::rf_reg_writable(r), "0x{r:02X} is not writable");
        }
    }

    #[test]
    fn text_helpers() {
        let mut b = [0u8; 28];
        let n = put_freq(&mut b, 0, 144_390_000);
        assert_eq!(&b[..n], b"144.390");
        let n = put_freq(&mut b, 0, 434_714_000);
        assert_eq!(&b[..n], b"434.714");
        let n = put_i(&mut b, 0, -105);
        assert_eq!(&b[..n], b"-105");
        let n = put_hex(&mut b, 0, 0x7E7E);
        assert_eq!(&b[..n], b"7E7E");
        let n = put_hex(&mut b, 0, 0x00AB);
        assert_eq!(&b[..n], b"00AB");
        let n = put_u(&mut b, 0, 0);
        assert_eq!(&b[..n], b"0");
    }
}
