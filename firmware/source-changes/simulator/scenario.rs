//! Scenario runner: drives a real app's `entry` through scripted key presses and time,
//! saves frames as PNG, and compares them with golden images.
//!
//!   sim scenario FILE.txt [--golden DIR] [--out DIR] [--update]
//!
//! Steps (one per line, `#` starts a comment):
//!   app NAME            fresh simulated radio, then Enter + Draw   (hamtools, foxhunt, ...)
//!   master HZ           the radio's tuned frequency when the app starts (default 146520000)
//!   signal HZ LEVEL TONE always | duty ON PERIOD   a transmitter on the air: raw RSSI LEVEL, CTCSS index TONE
//!                       (-1 none), either always on or on for ON s of every PERIOD s of radio time
//!   group N                 the channel group the radio is using (0 = ALL), for apps that follow it; before `app`
//!   chan N RXHZ [TXHZ] [NAME]   a stored channel (blank TX = receive-only). NAME `@ff` is the stock/CHIRP blank
//!                       name (twelve 0xFF bytes); no NAME leaves twelve zero bytes
//!                       master/signal/chan may appear before or after `app`; they are applied after the reset
//!   air HEX [HEX...]   the simulated FSK modem hears a sync, then these 16-bit words (hex), one per poll
//!                       (a package of several segments, like `chanmgr`, runs as in the firmware: `Chain` switches segment)
//!   airmode fsk|ffsk1224|ffsk1218|any   the tone pair of the signal `air` plays (REG_58 bits 12:10 000 / 100 / 111); the modem only
//!                       hears it with the matching RX mode. A GUESS in the model; `any` (default) hears every mode
//!   expect rfreg REG HEX   an RF chip register as the app left it (hex, e.g. `expect rfreg 58 1309`)
//!   expect handoff HEX...  the handoff buffer, as hex bytes (what a settings segment passed on with Chain)
//!   expect setting OFFSET V...  bytes of the settings record from OFFSET, decimal
//!   handoff GROUP ACTION [ROW]  preload the segment-switch buffer (settings app: GROUP index, ACTION 0=groups 1=items
//!                       2=about 3=grid locator, ROW the list row to come back to)
//!   handoff fm DECI        preload it as the core does for a broadcast frequency typed on the main screen (tenths of MHz)
//!   fmstation DECI NAME [DETAIL...]   one station in the FM app's flash table (`_` in NAME is a space, `-` is no name);
//!                       before or after `app`. The simulated tuner hears 90.1 94.5 98.1 101.1 104.5 MHz
//!   fmtable bad         a table whose header is damaged: the FM app must ignore it
//!   fmcore DECI...      the core's bare FM station list (what the FM app shows when there is no table)
//!   setting OFFSET V...   bytes of the settings record the app reads, written together through the core's clamp (settings app)
//!   coords LAT LON      observer position in decimal degrees (settings 5/6); `coords none` clears
//!   key K               single press (Press, Single, Release). K = 0-9 star pound menu exit up down vm ab band side1 side2
//!   hold K              long press: Press, Long, Release as the keypad sends them. Only up down exit star pound 0 vm band side1 side2 have one
//!                       (device/keypad.rs); any other key is an error here, because the radio would see a plain press
//!   repeat K N          up or down held past 1.5 s: Press, Long, N Repeat events, Release
//!   step MS             radio time advanced per tick by `wait` (default 5; larger = faster)
//!   wait SECS           run ticks + draws for SECS of radio time
//!   shot NAME           save NAME.png and compare with the golden of the same name
//!   expect exit|noexit  whether the LAST key made the app exit
//!   expect reset|noreset  whether the app has asked for a factory reset (the radio would reboot: nothing follows `reset`)
//!   expect backlight on|off   the app's backlight-hold request
//!   expect speaker on|off   the speaker switch after the core's key beep (Single/Long, beeps on, audio not open: OFF) has run
//!   expect cues N      speaker-only cue sequences played so far (api.play_cue); never keys PTT, checked automatically
//!   expect fmlive N     live stations in the FM table, as the app would read it; `expect fmtable none` for no valid table
//!   expect rfrefused N  RF register writes the core's allow-list refused
//!   expect captured N   records FSK PROBE left in its flash capture range
//!   expect vfo HZ CODE  what the app handed the core with api.save_master_vfo (tone code 0 = none); `expect vfo none` if nothing
//!   txshot NAME         the NEXT key or hold: also save the frame the app had on screen at the moment the transmitter was keyed
//!                       (the first delay while transmitting), as NAME.png compared with the golden like `shot`
//!   expect rxonly       (default on) fails the scenario if the app keyed PTT or the tone function
//!   allow tx            disables the rx-only safety check for this scenario
//! The same file is meant to drive the real radio through hil/ (keys via the injector,
//! `shot` via the webcam), so keep scenarios to what a person could do at the keypad.
use super::*;
use std::path::{Path, PathBuf};

fn lookup(name: &str) -> Option<Entry> {
    Some(match name {
        "hamtools" => hamtools::entry,
        "foxhunt" => foxhunt::entry,
        "fskprobe" => fskprobe::entry,
        "bandsweep" => bandsweep::entry,
        "rptfind" => rptfind::entry,
        "qtscan2" => qtscan2::entry,
        "search2" => search2::entry,
        "textapp" => textapp::entry,
        "searchqt" => sq,
        "spectrum" => |a, e| spectrum::app_entry(a, e),
        "fm" => fm::entry,
        "chanmgr" => |a, e| chain_run(
            &[(|a, e| chanmgr::list::app_entry(a, e)) as Entry, |a, e| chanmgr::edit::app_entry(a, e)],
            a,
            e,
        ),
        #[cfg(any(feature = "settings-menu", feature = "settings-radio", feature = "settings-system", feature = "settings-presets"))]
        "settings" => |a, e| settings::app_entry(a, e),
        _ => return None,
    })
}

/// A multi-segment package, as the firmware runs one: the event goes to the segment that is loaded, and when it answers
/// `Chain(n)` that segment is replaced by segment `n`, which gets `Enter` (the handoff buffer survives; each segment's
/// statics are its own because each is its own module). The `app` step resets this to segment 0.
static mut CHAIN_SEG: usize = 0;

fn chain_run(segs: &[Entry], api: &Api, ev: AppEvent) -> AppResult {
    let mut r = segs[unsafe { CHAIN_SEG }](api, ev);
    let mut hops = 0;
    while let AppResult::Chain(n) = r {
        hops += 1;
        assert!(hops <= 4 && (n as usize) < segs.len(), "bad segment switch to {n}");
        unsafe { CHAIN_SEG = n as usize };
        r = segs[n as usize](api, AppEvent::Enter); // as `handle_result` does: no Leave, a fresh Enter
    }
    r
}

fn key_id(k: &str) -> Option<u8> {
    Some(match k {
        "star" => 10,
        "pound" => 11,
        "menu" => 12,
        "exit" => 13,
        "up" => 14,
        "down" => 15,
        "vm" => 16,
        "ab" => 17,
        "band" => 18,
        "side1" => 19,
        "side2" => 20,
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => d.as_bytes()[0] - b'0',
        _ => return None,
    })
}

// ---- PNG (uncompressed deflate, deterministic) ------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

pub fn png_encode(w: usize, h: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w * 3..(y + 1) * w * 3]);
    }
    let mut z = vec![0x78, 0x01];
    let mut it = raw.chunks(65535).peekable();
    while let Some(c) = it.next() {
        z.push(if it.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Decodes only PNGs made by `png_encode` (stored blocks, filter 0). None otherwise.
fn png_decode_own(png: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let mut p = 8;
    let mut idat = Vec::new();
    while p + 8 <= png.len() {
        let len = u32::from_be_bytes(png[p..p + 4].try_into().ok()?) as usize;
        if &png[p + 4..p + 8] == b"IDAT" {
            idat.extend_from_slice(png.get(p + 8..p + 8 + len)?);
        }
        p += 12 + len;
    }
    let mut z = idat.get(2..)?;
    let mut raw = Vec::new();
    loop {
        let (hdr, rest) = z.split_first()?;
        if hdr & 6 != 0 {
            return None; // compressed block: not ours
        }
        let len = u16::from_le_bytes(rest.get(0..2)?.try_into().ok()?) as usize;
        raw.extend_from_slice(rest.get(4..4 + len)?);
        z = &rest[4 + len..];
        if hdr & 1 != 0 {
            break;
        }
    }
    let mut rgb = Vec::new();
    for y in 0..h {
        let row = raw.get(y * (w * 3 + 1)..(y + 1) * (w * 3 + 1))?;
        if row[0] != 0 {
            return None;
        }
        rgb.extend_from_slice(&row[1..]);
    }
    Some(rgb)
}

pub(crate) fn frame_rgb() -> Vec<u8> {
    let mut out = Vec::with_capacity(W * H * 3);
    for &p in &sim().fb {
        out.push((((p >> 11) & 0x1F) * 255 / 31) as u8);
        out.push((((p >> 5) & 0x3F) * 255 / 63) as u8);
        out.push(((p & 0x1F) * 255 / 31) as u8);
    }
    out
}

// ---- runner ----------------------------------------------------------------------------

pub struct Opts {
    pub file: PathBuf,
    pub golden: Option<PathBuf>,
    pub out: PathBuf,
    pub update: bool,
}

pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut file = None;
    let (mut golden, mut out, mut update) = (None, PathBuf::from("/tmp/sim-scenarios"), false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--golden" => { i += 1; golden = Some(PathBuf::from(args.get(i).ok_or("--golden needs a DIR")?)); }
            "--out" => { i += 1; out = PathBuf::from(args.get(i).ok_or("--out needs a DIR")?); }
            "--update" => update = true,
            f if file.is_none() => file = Some(PathBuf::from(f)),
            other => return Err(format!("unexpected argument {other}")),
        }
        i += 1;
    }
    Ok(Opts { file: file.ok_or("usage: sim scenario FILE [--golden DIR] [--out DIR] [--update]")?, golden, out, update })
}

/// Radio conditions set up by the scenario; re-applied each time `app` resets the simulated radio.
type Setup = Box<dyn Fn()>;

fn bcd(deci_hz: u32) -> [u8; 4] {
    let b = |v: u32| (((v % 100) / 10) << 4 | (v % 10)) as u8;
    [b(deci_hz), b(deci_hz / 100), b(deci_hz / 10_000), b(deci_hz / 1_000_000)]
}

fn channel_record(rx: u32, tx: Option<u32>, name: &str) -> [u8; 32] {
    let mut r = [0u8; 32];
    r[0..4].copy_from_slice(&bcd(rx / 10));
    r[4..8].copy_from_slice(&tx.map_or([0xFF; 4], |t| bcd(t / 10)));
    if name == "@ff" {
        r[20..32].fill(0xFF); // what stock firmware and CHIRP store for an unnamed channel
        return r;
    }
    for (i, c) in name.bytes().take(12).enumerate() {
        r[20 + i] = c;
    }
    r
}

struct Run<'a> {
    o: &'a Opts,
    master: u32,
    setup: Vec<Setup>,
    name: String,
    app: Option<Entry>,
    step_ms: u32,
    last_exit: bool,
    rx_only: bool,
    tx_shot: Option<String>,
    pass: u32,
    fail: u32,
}

impl Run<'_> {
    fn check(&mut self, ok: bool, what: &str) {
        if ok { self.pass += 1; println!("  ok   {what}"); } else { self.fail += 1; println!("  FAIL {what}"); }
    }

    fn shot(&mut self, shot: &str) {
        let rgb = frame_rgb();
        self.shot_rgb(shot, rgb);
    }

    fn shot_rgb(&mut self, shot: &str, rgb: Vec<u8>) {
        let png = png_encode(W, H, &rgb);
        let dir = self.o.out.join(&self.name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{shot}.png")), &png).unwrap();
        let Some(g) = &self.o.golden else {
            return self.check(true, &format!("shot {shot} (saved, no golden dir)"));
        };
        let gpath = g.join(&self.name).join(format!("{shot}.png"));
        if self.o.update {
            std::fs::create_dir_all(gpath.parent().unwrap()).unwrap();
            std::fs::write(&gpath, &png).unwrap();
            return self.check(true, &format!("shot {shot} (golden updated)"));
        }
        match std::fs::read(&gpath) {
            Err(_) => self.check(false, &format!("shot {shot}: no golden at {} (run with --update)", gpath.display())),
            Ok(want) if want == png => self.check(true, &format!("shot {shot} matches golden")),
            Ok(want) => {
                let n = png_decode_own(&want, W, H)
                    .map(|g| g.chunks(3).zip(rgb.chunks(3)).filter(|(a, b)| a != b).count());
                let msg = match n { Some(n) => format!("{n} pixels differ"), None => "differs".into() };
                self.check(false, &format!("shot {shot}: {msg} from golden (actual in {})", dir.display()));
            }
        }
    }

    /// The real main loop draws every tick; state set from outside needs one draw to show.
    fn redraw(&mut self) {
        if let Some(app) = self.app {
            app(&API, AppEvent::Draw);
        }
    }

    fn wait(&mut self, secs: f64) {
        let app = self.app.expect("`app` must come first");
        let end = sim().now.wrapping_add((secs * 10_000.0) as u32);
        let step = self.step_ms * 10;
        let mut last = sim().now;
        while sim().now.wrapping_sub(end) > u32::MAX / 2 {
            sim().now = sim().now.wrapping_add(step);
            let dt = sim().now.wrapping_sub(last);
            last = sim().now;
            app(&API, AppEvent::Tick { dt_100us: dt });
            app(&API, AppEvent::Draw);
        }
    }

    fn line(&mut self, ln: usize, text: &str) {
        let t: Vec<&str> = text.split('#').next().unwrap().split_whitespace().collect();
        if t.is_empty() { return; }
        println!(" {ln:>3}: {}", t.join(" "));
        match t.as_slice() {
            ["app", n] => {
                let Some(e) = lookup(n) else { return self.check(false, &format!("unknown app {n}")) };
                reset(self.master);
                chip_reset();   // the simulated FSK modem (TEXT)
                nor_clear();    // a blank SPI flash for apps that save data
                fm_reset();     // the simulated FM tuner
                unsafe { TONE_ON_CALLS = 0; CUES.clear(); RF_WRITES_REFUSED = 0; }
                for f in &self.setup {
                    f();
                }
                self.app = Some(e);
                unsafe { CHAIN_SEG = 0 };
                e(&API, AppEvent::Enter);
                e(&API, AppEvent::Draw);
            }
            ["group", n] => match n.parse::<u8>() {
                Ok(g) => unsafe { crate::GROUP = g },
                Err(_) => self.check(false, "group needs a group index"),
            },
            ["master", hz] => match hz.parse::<u32>() {
                Ok(h) => self.master = h,
                Err(_) => self.check(false, "master needs a frequency in Hz"),
            },
            ["signal", hz, level, tone, mode @ ..] => {
                let (Ok(hz), Ok(level), Ok(tone)) = (hz.parse::<u32>(), level.parse::<u16>(), tone.parse::<i32>()) else {
                    return self.check(false, "signal needs HZ LEVEL TONE");
                };
                let duty = match mode {
                    ["always"] => None,
                    ["duty", on, per] => match (on.parse::<u32>(), per.parse::<u32>()) {
                        (Ok(on), Ok(per)) if per > 0 => Some((on, per)),
                        _ => return self.check(false, "duty needs ON PERIOD seconds"),
                    },
                    _ => return self.check(false, "signal needs `always` or `duty ON PERIOD`"),
                };
                let f: Setup = Box::new(move || match duty {
                    None => add(hz, level, tone, |_| true),
                    Some((on, per)) => add(hz, level, tone, move |t| (t % (per * 10_000)) < on * 10_000),
                });
                if self.app.is_some() { f(); }
                self.setup.push(f);
            }
            ["air", words @ ..] => {
                let Ok(w) = words.iter().map(|h| u16::from_str_radix(h, 16)).collect::<Result<Vec<_>, _>>() else {
                    return self.check(false, "air needs hex words");
                };
                let c = chip();
                c.script.push_back(AirEvent::Sync);
                c.script.extend(w.into_iter().map(AirEvent::Word));
            }
            ["airmode", m] => {
                chip().hears = match *m {
                    "fsk" => Some(0b000),
                    "ffsk1224" => Some(0b100),
                    "ffsk1218" => Some(0b111),
                    "any" => None,
                    _ => return self.check(false, "airmode needs fsk, ffsk1224, ffsk1218 or any"),
                };
            }
            ["expect", "rfreg", reg, val] => {
                let (Ok(r), Ok(v)) = (usize::from_str_radix(reg, 16), u16::from_str_radix(val, 16)) else {
                    return self.check(false, "expect rfreg needs REG VALUE in hex");
                };
                let got = chip().regs.get(r).copied().unwrap_or(0xFFFF);
                self.check(got == v, &format!("RF register {r:02X} is {v:04X} (got {got:04X})"));
            }
            ["chan", n, rx, rest @ ..] => {
                let (Ok(n), Ok(rx)) = (n.parse::<u16>(), rx.parse::<u32>()) else {
                    return self.check(false, "chan needs N RXHZ");
                };
                let (tx, name) = match rest {
                    [] => (None, ""),
                    [t] if t.parse::<u32>().is_ok() => (t.parse().ok(), ""),
                    [name] => (None, *name),
                    [t, name, ..] => (t.parse().ok(), *name),
                };
                let name = name.to_string();
                let f: Setup = Box::new(move || { sim().chans.insert(n, channel_record(rx, tx, &name)); });
                if self.app.is_some() { f(); }
                self.setup.push(f);
            }
            ["fmstation", deci, name, detail @ ..] => {
                let Ok(d) = deci.parse::<u16>() else { return self.check(false, "fmstation needs tenths of a MHz") };
                let name = if *name == "-" { String::new() } else { name.replace('_', " ") };
                let detail = detail.join(" ");
                let f: Setup = Box::new(move || fm_table_add(d, name.as_bytes(), detail.as_bytes()));
                if self.app.is_some() { f(); }
                self.setup.push(f);
            }
            ["fmtable", "bad"] => {
                let f: Setup = Box::new(|| {
                    fm_table_add(901, b"HIDDEN", b"");
                    nor()[fm::TBL_ADDR as usize..fm::TBL_ADDR as usize + 4].copy_from_slice(b"FMSX");
                });
                if self.app.is_some() { f(); }
                self.setup.push(f);
            }
            ["fmcore", list @ ..] => {
                let Ok(l) = list.iter().map(|d| d.parse::<u16>()).collect::<Result<Vec<_>, _>>() else {
                    return self.check(false, "fmcore needs tenths of a MHz");
                };
                let f: Setup = Box::new(move || fm_tuner().core_list = l.clone());
                if self.app.is_some() { f(); }
                self.setup.push(f);
            }
            ["handoff", "fm", deci] => match deci.parse::<u16>() {
                Ok(d) => unsafe { HANDOFF = d.to_le_bytes().to_vec() }, // as the core does for a typed broadcast frequency
                _ => self.check(false, "handoff fm needs tenths of a MHz"),
            },
            ["handoff", group, action] | ["handoff", group, action, _] => {
                let row = t.get(3).map_or(Some(0), |r| r.parse::<u8>().ok());
                match (group.parse::<u8>(), action.parse::<u8>(), row) {
                    // start a later settings segment as Chain would: group (or the row a view starts on), action, row to return to
                    (Ok(g), Ok(a), Some(r)) => unsafe { HANDOFF = vec![0xA5, g, a, r] },
                    _ => self.check(false, "handoff needs GROUP ACTION [ROW] numbers"),
                }
            }
            // one byte of the settings record (offsets as in `Settings` in flash_map.rs), to start from a record that is
            // not the simulator's all-zero one
            ["setting", off, vals @ ..] if !vals.is_empty() => match (off.parse::<usize>(), vals.iter().map(|v| v.parse::<u8>()).collect::<Result<Vec<_>, _>>()) {
                (Ok(o), Ok(v)) if o + v.len() <= 168 => unsafe {
                    // the bytes go in together and through the same clamp as a committed record (Settings::from_bytes), so a
                    // value the core would change is refused here (a position is four bytes: give them in one step)
                    let mut rec = SETTINGS;
                    rec[o..o + v.len()].copy_from_slice(&v);
                    normalize_settings(&mut rec);
                    self.check(rec[o..o + v.len()] == v[..], &format!("setting {o} {v:?}: the core would store {:?} (Settings::from_bytes clamps it)", &rec[o..o + v.len()]));
                    SETTINGS = rec;
                },
                _ => self.check(false, "setting needs a byte offset and values (0-255) inside the 168-byte record"),
            },
            ["coords", "none"] => { set_coords(None); self.redraw() }
            ["coords", la, lo] => match (la.parse::<f64>(), lo.parse::<f64>()) {
                (Ok(a), Ok(o)) => {
                    set_coords(Some(((a * 1e5).round() as i32, (o * 1e5).round() as i32)));
                    self.redraw();
                }
                _ => self.check(false, "coords needs two decimal degrees"),
            },
            ["key", k] | ["hold", k] => {
                let (Some(app), Some(id)) = (self.app, key_id(k)) else {
                    return self.check(false, &format!("bad key step `{}`", t.join(" ")));
                };
                if t[0] == "hold" && !keypad_long(id) {
                    // device/keypad.rs: this key never sends a Long event, so the radio would read the hold as a plain press
                    return self.check(false, &format!("the keypad sends no long press for `{k}`: `{}`", t.join(" ")));
                }
                // The key runs on a helper thread: a factory reset never returns (it reboots the radio), so the
                // simulated call parks its thread and this one carries on, having seen the call.
                let before = crate::FACTORY_CALLS.load(std::sync::atomic::Ordering::SeqCst);
                let is_key = t[0] == "key";
                let worker = std::thread::spawn(move || if is_key { press(app, id) } else { hold(app, id) });
                loop {
                    if worker.is_finished() {
                        self.last_exit = worker.join().unwrap_or(false);
                        break;
                    }
                    if crate::FACTORY_CALLS.load(std::sync::atomic::Ordering::SeqCst) != before {
                        self.last_exit = false;
                        break; // the parked thread is left behind; nothing more is expected of this radio
                    }
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
                if let Some(name) = self.tx_shot.take() {
                    match unsafe { TX_CAPTURE.take() } {
                        Some(Some(rgb)) => self.shot_rgb(&name, rgb),
                        _ => self.check(false, &format!("txshot {name}: the transmitter was never keyed")),
                    }
                }
            }
            ["repeat", k, n] => {
                let (Some(app), Some(id), Ok(n)) = (self.app, key_id(k), n.parse::<u32>()) else {
                    return self.check(false, &format!("bad repeat step `{}`", t.join(" ")));
                };
                if !keypad_repeat(id) {
                    return self.check(false, &format!("the keypad repeats only up and down, not `{k}`"));
                }
                self.last_exit = hold_repeat(app, id, n);
            }
            ["step", ms] => self.step_ms = ms.parse().unwrap_or(5).max(1),
            ["wait", s] => match s.parse::<f64>() {
                Ok(s) => self.wait(s),
                Err(_) => self.check(false, "wait needs seconds"),
            },
            ["shot", n] => self.shot(n),
            ["expect", "exit"] => { let v = self.last_exit; self.check(v, "last key exited the app") }
            ["expect", "noexit"] => { let v = !self.last_exit; self.check(v, "last key did not exit the app") }
            ["expect", "reset"] => {
                let n = crate::FACTORY_CALLS.load(std::sync::atomic::Ordering::SeqCst);
                self.check(n > 0, "the app asked for a factory reset")
            }
            // what the app left for the next segment (settings app): the bytes of the handoff buffer, in hex
            ["expect", "handoff", bytes @ ..] => {
                let want = bytes.iter().map(|b| u8::from_str_radix(b, 16)).collect::<Result<Vec<_>, _>>();
                let got = unsafe { HANDOFF.clone() };
                self.check(want.as_ref().ok() == Some(&got), &format!("handoff buffer is {} (got {:02X?})", bytes.join(" "), got));
            }
            // bytes of the settings record, from OFFSET, in decimal
            ["expect", "setting", off, vals @ ..] => {
                let off = off.parse::<usize>().unwrap_or(usize::MAX);
                let want = vals.iter().map(|v| v.parse::<u8>()).collect::<Result<Vec<_>, _>>();
                let got = unsafe { SETTINGS.get(off..off + vals.len()).map(|g| g.to_vec()) };
                self.check(got.is_some() && want.as_ref().ok() == got.as_ref(), &format!("settings bytes {off}.. are {} (got {got:?})", vals.join(" ")));
            }
            ["expect", "noreset"] => {
                let n = crate::FACTORY_CALLS.load(std::sync::atomic::Ordering::SeqCst);
                self.check(n == 0, "the app did not ask for a factory reset")
            }
            ["expect", "backlight", v] => {
                let want = *v == "on";
                let got = unsafe { BACKLIGHT_HOLD };
                self.check(got == want, &format!("backlight hold is {v}"));
            }
            ["expect", "cues", n] => {
                let got = unsafe { CUES.len() };
                self.check(n.parse::<usize>().ok() == Some(got), &format!("{n} speaker cue(s) played (got {got})"));
            }
            ["expect", "speaker", v @ ("on" | "off")] => {
                // the speaker switch as the core would leave it: after the key beep (a Single or Long key event with beeps
                // on and audio not open switches it OFF), the app's own set_speaker / set_af_out, and the squelch poll
                let on = unsafe { SPEAKER_ON };
                self.check(on == (*v == "on"), &format!("speaker {v} (it is {})", if on { "on" } else { "off" }));
            }
            ["expect", "captured", n] => {
                // FSK PROBE's capture as probe_capture.py would read it: a header, then records until an erased one
                let nor = nor();
                let got = if &nor[0x1B_0000..0x1B_0008] == b"FSKCAP3\0" {
                    nor[0x1B_0100..0x1D_0000].chunks(8).take_while(|r| r[6] != 0xFF).count()
                } else {
                    usize::MAX
                };
                self.check(n.parse::<usize>().ok() == Some(got), &format!("{n} captured record(s) in flash (got {got})"));
            }
            ["expect", "fmtable", "none"] => {
                let got = fm_table_live().is_none();
                self.check(got, "no valid FM station table in flash");
            }
            ["expect", "fmlive", n] => {
                let got = fm_table_live();
                self.check(got.is_some() && n.parse::<usize>().ok() == got, &format!("{n} live FM station(s) in flash (got {got:?})"));
            }
            ["expect", "rfrefused", n] => {
                let got = unsafe { RF_WRITES_REFUSED };
                self.check(n.parse::<u32>().ok() == Some(got), &format!("{n} refused RF register write(s) (got {got})"));
            }
            ["expect", "vfo", "none"] => {
                let got = sim().vfo_saved;
                self.check(got.is_none(), &format!("nothing handed to the VFO (got {got:?})"));
            }
            ["expect", "vfo", hz, code] => {
                let want = hz.parse::<u32>().ok().zip(code.parse::<u16>().ok());
                let got = sim().vfo_saved;
                self.check(want.is_some() && got == want, &format!("VFO handed {hz} Hz, tone code {code} (got {got:?})"));
            }
            ["txshot", n] => {
                unsafe { TX_CAPTURE = Some(None) };
                self.tx_shot = Some(n.to_string());
            }
            ["allow", "tx"] => self.rx_only = false,
            _ => self.check(false, &format!("unrecognised step `{}`", t.join(" "))),
        }
    }
}

/// Puts one station into the FM table the way the host tool does (header first, then the next free record).
fn fm_table_add(deci: u16, name: &[u8], detail: &[u8]) {
    let base = fm::TBL_ADDR as usize;
    let nor = nor();
    if &nor[base..base + 4] != b"FMST" {
        let mut h = [0xFFu8; 32];
        h[..4].copy_from_slice(b"FMST");
        h[4] = 1;
        h[6..8].copy_from_slice(&1u16.to_le_bytes());
        h[31] = 0xA5;
        nor[base..base + 32].copy_from_slice(&h);
    }
    let slot = (1..=127).find(|&i| nor[base + i * 32 + 31] == 0xFF).expect("FM table full");
    nor[base + slot * 32..base + slot * 32 + 32].copy_from_slice(&fm::make_rec(deci, name, detail));
}

/// Live stations in the FM table's active half, or None when neither half has a valid header.
fn fm_table_live() -> Option<usize> {
    let nor = nor();
    let base = fm::TBL_ADDR as usize;
    let gen = |half: usize| {
        let h = &nor[base + half * 4096..base + half * 4096 + 32];
        (&h[..4] == b"FMST" && h[4] == 1 && h[31] == 0xA5).then(|| u16::from_le_bytes([h[6], h[7]]))
    };
    let half = match (gen(0), gen(1)) {
        (None, None) => return None,
        (Some(_), None) => 0,
        (None, Some(_)) => 1,
        (Some(a), Some(b)) => if (a.wrapping_sub(b) as i16) >= 0 { 0 } else { 1 },
    };
    let at = base + half * 4096;
    Some((1..=127).filter(|&i| {
        let r = &nor[at + i * 32..at + i * 32 + 32];
        let f = u16::from_le_bytes([r[0], r[1]]);
        r[31] == 0xA5 && (fm::LO..=fm::HI).contains(&f)
    }).count())
}

/// Runs one scenario file; returns the number of failures.
pub fn run_file(o: &Opts) -> u32 {
    let name = o.file.file_stem().unwrap().to_string_lossy().to_string();
    let text = std::fs::read_to_string(&o.file).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", o.file.display());
        std::process::exit(2)
    });
    println!("scenario {name}");
    let mut r = Run { o, master: 146_520_000, setup: Vec::new(), name, app: None, step_ms: 5, last_exit: false, rx_only: true, tx_shot: None, pass: 0, fail: 0 };
    for (i, l) in text.lines().enumerate() {
        r.line(i + 1, l);
    }
    if r.rx_only {
        let (tone, ptt) = (unsafe { TONE_ON_CALLS }, sim().ptt_calls);
        r.check(tone == 0, "never called the keying tone function");
        r.check(ptt == 0, "never keyed PTT");
    }
    println!("  {} passed, {} failed", r.pass, r.fail);
    r.fail
}

#[allow(dead_code)]
fn _unused(_: &Path) {}
