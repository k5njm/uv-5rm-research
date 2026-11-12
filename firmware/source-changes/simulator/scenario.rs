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
//!   handoff GROUP ACTION   preload the segment-switch buffer (settings app: GROUP index, ACTION 0=groups 1=items)
//!   handoff fm DECI        preload it as the core does for a broadcast frequency typed on the main screen (tenths of MHz)
//!   fmstation DECI NAME [DETAIL...]   one station in the FM app's flash table (`_` in NAME is a space, `-` is no name);
//!                       before or after `app`. The simulated tuner hears 90.1 94.5 98.1 101.1 104.5 MHz
//!   fmtable bad         a table whose header is damaged: the FM app must ignore it
//!   fmcore DECI...      the core's bare FM station list (what the FM app shows when there is no table)
//!   coords LAT LON      observer position in decimal degrees (settings 5/6); `coords none` clears
//!   key K               single press. K = 0-9 star pound menu exit up down ab
//!   hold K              long press
//!   step MS             radio time advanced per tick by `wait` (default 5; larger = faster)
//!   wait SECS           run ticks + draws for SECS of radio time
//!   shot NAME           save NAME.png and compare with the golden of the same name
//!   expect exit|noexit  whether the LAST key made the app exit
//!   expect backlight on|off   the app's backlight-hold request
//!   expect cues N       speaker-only cue sequences played so far (api.play_cue); never keys PTT, checked automatically
//!   expect fmlive N     live stations in the FM table, as the app would read it; `expect fmtable none` for no valid table
//!   expect rfrefused N  RF register writes the core's allow-list refused
//!   expect captured N   records FSK PROBE left in its flash capture range
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
        "chanmgr" => |a, e| chanmgr::app_entry(a, e),
        #[cfg(any(feature = "settings-menu", feature = "settings-radio", feature = "settings-system"))]
        "settings" => |a, e| settings::app_entry(a, e),
        _ => return None,
    })
}

fn key_id(k: &str) -> Option<u8> {
    Some(match k {
        "star" => 10,
        "pound" => 11,
        "menu" => 12,
        "exit" => 13,
        "up" => 14,
        "down" => 15,
        "ab" => 17,
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

fn frame_rgb() -> Vec<u8> {
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
    pass: u32,
    fail: u32,
}

impl Run<'_> {
    fn check(&mut self, ok: bool, what: &str) {
        if ok { self.pass += 1; println!("  ok   {what}"); } else { self.fail += 1; println!("  FAIL {what}"); }
    }

    fn shot(&mut self, shot: &str) {
        let rgb = frame_rgb();
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
            ["handoff", group, action] => match (group.parse::<u8>(), action.parse::<u8>()) {
                (Ok(g), Ok(a)) => unsafe { HANDOFF = vec![0xA5, g, a] }, // start a later settings segment as Chain would
                _ => self.check(false, "handoff needs GROUP ACTION numbers"),
            },
            ["coords", "none"] => { unsafe { COORDS = None }; self.redraw() }
            ["coords", la, lo] => match (la.parse::<f64>(), lo.parse::<f64>()) {
                (Ok(a), Ok(o)) => {
                    unsafe { COORDS = Some(((a * 1e5).round() as i32, (o * 1e5).round() as i32)) };
                    self.redraw();
                }
                _ => self.check(false, "coords needs two decimal degrees"),
            },
            ["key", k] | ["hold", k] => {
                let (Some(app), Some(id)) = (self.app, key_id(k)) else {
                    return self.check(false, &format!("bad key step `{}`", t.join(" ")));
                };
                self.last_exit = if t[0] == "key" { press(app, id) } else { hold(app, id) };
            }
            ["step", ms] => self.step_ms = ms.parse().unwrap_or(5).max(1),
            ["wait", s] => match s.parse::<f64>() {
                Ok(s) => self.wait(s),
                Err(_) => self.check(false, "wait needs seconds"),
            },
            ["shot", n] => self.shot(n),
            ["expect", "exit"] => { let v = self.last_exit; self.check(v, "last key exited the app") }
            ["expect", "noexit"] => { let v = !self.last_exit; self.check(v, "last key did not exit the app") }
            ["expect", "backlight", v] => {
                let want = *v == "on";
                let got = unsafe { BACKLIGHT_HOLD };
                self.check(got == want, &format!("backlight hold is {v}"));
            }
            ["expect", "cues", n] => {
                let got = unsafe { CUES.len() };
                self.check(n.parse::<usize>().ok() == Some(got), &format!("{n} speaker cue(s) played (got {got})"));
            }
            ["expect", "captured", n] => {
                // FSK PROBE's capture as probe_capture.py would read it: a header, then records until an erased one
                let nor = nor();
                let got = if &nor[0x1B_0000..0x1B_0008] == b"FSKCAP1\0" {
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
    let mut r = Run { o, master: 146_520_000, setup: Vec::new(), name, app: None, step_ms: 5, last_exit: false, rx_only: true, pass: 0, fail: 0 };
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
