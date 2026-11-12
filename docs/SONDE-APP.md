# Radiosonde app (design note, not built)

Status 2026-11-12: **proposal.** Nothing here runs yet. What is built is FSK PROBE (launcher slot 8), preset for the
test described under "Step 1", with a capture mode (`#`) and `probe_capture.py` to download it for step 2. The first capture on noise exposed setup faults in the probe, fixed on branch `fix/fskprobe-registers` (HARDWARE.md section 6); the tests below are for that fixed build, which is unflashed.

## Goal
Receive and decode a Vaisala RS41 radiosonde on the UV-5RM and show it the way
[rdz_ttgo_sonde](https://github.com/dl9rdz/rdz_ttgo_sonde) does on its OLED. The owner has one of those receivers, so it
is both the reference for the screen and a second receiver to compare against. Local target: the NWS Fort Worth sonde,
nominally 404.800 MHz (`channels/sonde.csv`).

## What the reference decoder does (read from its source, not copied)
From `RX_FSK/src/RS41.cpp` of rdz_ttgo_sonde (branch `dev2`, read 2026-11-12):

| Fact | Value |
|---|---|
| Speed | 4800 baud, two-level FSK |
| Sync | the 8-byte frame header `08 6D 53 88 44 69 48 1F`, as the SX1278 shifts it in (most-significant bit first) |
| Frame | 320 bytes including the header (a longer 518-byte variant exists; it tries the short Reed-Solomon layout first) |
| After reception | reverse the bits of every byte (the sonde sends least-significant bit first), then XOR with a repeating 64-byte mask |
| Error correction | Reed-Solomon over the frame (two interleaved codewords), then a CRC16 per sub-block |
| Position | ECEF X, Y, Z in centimetres and velocities, converted to latitude, longitude and height (WGS84) |
| Receiver | SX1278 FSK mode with automatic frequency correction and a 4-byte sync match |

This confirms the sync preset FSK PROBE uses on key `8` (`086D 5388`, the header's first four bytes, most-significant bit
first). Key `9` is the same header least-significant bit first (`10B6 CA11`), in case the FD6818B shifts the other way.

## What is different on this radio
* The SX1278 delivers whole frames from a 64-byte FIFO with interrupts. The FD6818B's receive FIFO holds **8 words**
  (Fudan register document), and the app polls it. A 320-byte frame at 4800 baud arrives in about 0.53 s, so the FIFO
  fills every 27 ms. Whether an overlay app's tick is fast enough is unknown.
* The FD6818B's sync word, bit order, and FSK deviation/bandwidth settings for 4800 baud are unverified on this board.
* No automatic frequency correction is known for this chip's FSK mode. A sonde's carrier drifts by a few kilohertz.
* The app arena is 8,192 bytes. Reed-Solomon (255,231) decoding with its tables, the 64-byte mask, a 320-byte frame
  buffer and the WGS84 conversion all have to fit, probably as two segments (receive, then decode and display).
* No floating-point unit and no `f64` library in an app today; the position conversion needs fixed-point or soft-float.

## Steps, each one deciding whether the next is worth doing
1. **Can the chip lock to an RS41?** FSK PROBE during a launch (the NWS launches around 11:00 and 23:00 UTC): tune
   404.800, keys `4` (4800 baud) and `8` or `9`. SYNCS rising means the modem sees the header. WORDS rising in step
   means data follows. Run rdz_ttgo_sonde beside it: if it decodes and the probe shows nothing, the signal was there.
2. **Can the app keep up with the FIFO?** Press `#` in FSK PROBE during the launch (up to 16,352 words, about 55 s),
   then `probe_capture.py PORT launch.bin`. Its summary answers the timing question directly: the time between words,
   how many words one tick had to take, and the bit rate delivered against 4800. Compare the words with the same
   frames from rdz_ttgo_sonde (its serial log) after bit reversal and unmasking, and with an SDR recording of the same
   minute (the owner has an SDR): that recording is the reference independent of both receivers, and can be replayed
   into a decoder on the host without waiting for a launch. Keep the captures as test data for step 3.
3. **Decoder on the host.** `logic.rs`, unit-tested against recorded RS41 frames, before anything reaches the radio:
   header check, unmask, CRC per block, then Reed-Solomon if it fits.
4. **Screen**, after rdz_ttgo_sonde's receiver view: ID, latitude and longitude, altitude, climb rate, signal
   strength, and its row of marks for the last 18 frames (received, missed, failed checksum). Distance and bearing to
   the sonde need the operator's position (SETTINGS has latitude and longitude). Its 400-406 MHz spectrum view is
   close to what SPECTRUM already does.

## Licence
rdz_ttgo_sonde is GPL-2.0-or-later (its `Licenses/README`); this project's firmware is GPL-3.0. Code under "GPL-2.0 or
later" may be used in a GPL-3.0 work, with attribution, but individual files carry their own identifiers and its
`Licenses/Exceptions` file lists exceptions. So far nothing has been copied: the table above is facts read from the source.
Check the specific file's identifier before porting any code, and credit it in HARDWARE.md or REFERENCES.md.

## On-radio tests for the fixed FSK PROBE (owner)

Settings: FSK PROBE starts on 404.800 MHz, 4800 baud, sync `086D 5388`, options `LEN316 TH1 INV- BW2.4 S4`.
`AB` picks an option (the yellow one), `0` changes it. Options cannot change while recording. Every capture records them
in its header and `probe_capture.py` prints them. Receive only; nothing here transmits.

### (a) On noise or interference at 404.800 MHz, any time

1. Flash the image from `scripts/dev.sh release` whose hash matches the pin recorded by `scripts/dev.sh check`. Open FSK PROBE.
   Press `MENU` (modem on). Check the peek line (`UP`/`DOWN`) shows REG 59 = 1008 and REG 5D = 3B20.
2. Press `#`, wait 30 s, press `#`, leave the app, `python3 firmware/source-changes/probe_capture.py PORT noise-a.bin`.
   Expect `options 0x10 ... sync 4 bytes` in the summary.
3. **Fault 1, sync width.** With S4, a noise sync should be essentially never (a 32-bit match, about 1 in 4e9 per bit position).
   Zero syncs in 30 s is consistent with the fix. Then press `AB` four times, `0` (S2), `#` for 30 s again (`noise-b.bin`):
   a 16-bit match should give a few syncs in 30 s (4800 x 30 / 65,536 = 2.2 on random bits; more if the noise is
   structured). Syncs with S2 and none with S4 **confirms** fault 1; syncs with S4 at a similar rate **refutes** that
   bit 3 does anything on this chip (and would point to a tolerant match).
4. **Fault 2, streaming.** A noise sync starts a frame. With S2, look at the frames in the summary: a frame should show many
   words (up to 159) about 3.3 ms apart ("time between words: median 3.3 ms"), one word per record. That **confirms** the
   clear-bit explanation. A sync followed by a done flag and no words (the first capture's signature) **refutes** it: then
   the almost-full interrupt does not behave as assumed. Then try TH (`AB`, `0`; 4 words per interrupt) and repeat: the
   median gap between records stays 3.3 ms but "most words read in one interrupt" becomes 4 (more if a tail is read).
5. **Fault 3, length.** With S2 the screen shows LEN318 (the chip is told 318 bytes): sync to end should be about
   530 ms and `frames ended by count` (the chip's done flag at the same moment). A done flag at about 108-120 ms regardless
   of the LEN setting **refutes** that the top 3 bits of REG_5D work. Then set LEN 256: the chip's done should come at about
   427 ms, and if the words continue to the frame length, "frames ended by count" counts them (the chip keeps streaming); if
   they stop at 128 the frames show "cut off". With LEN 2048 there is no done flag and frames end by count.
6. Also compare the sync-to-done time of the first capture (119.7 ms for a 65-byte request) with the new ones: whether the
   11 ms excess scales with the length (baud error) or stays constant (sync bytes or latency).

### (b) During a real launch (about 11:00 and 23:00 UTC, NWS Fort Worth)

1. Beforehand: rdz_ttgo_sonde on 404.800 logging to serial (or its web log) and the SDR recording 404.800 +/- 100 kHz
   with the start time noted. Put all three antennas within a few metres and set the computer clock.
2. FSK PROBE with defaults, `MENU`, `#`, as soon as rdz shows an RS41 frame. Leave it up to 55 s, `#`, then download.
3. Compare, second by second: rdz's decoded frames and times; the SDR demodulation (the header `08 6D 53 88` in the bits,
   deviation, polarity); the probe's summary (syncs, `as sent` or `inverted`, words per frame, frames ended by count).
   * Syncs with 158-word frames: the chip locks. The words should equal frame bytes 5 to 320 (after the sync) before bit
     reversal and unmasking; the first word is `6944` or `4469` (bytes `44 69`), which settles the byte order guess.
   * Syncs `inverted` only, or none: repeat with INV+; try sync preset `9` (bit order) and BW1.2.
   * No syncs while rdz decodes: the modem does not lock (bandwidth, deviation or crystal). Note RSSI and the SDR deviation.
   * Frames shorter than 158 words: try LEN and TH, or the tick cannot keep up (gaps over 30 ms, "most words in one
     interrupt" at 8).
4. Repeat once with TH 4 and once with LEN 256 to see which reading the chip follows. Keep every `.bin` and the SDR file.
