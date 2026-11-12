# Radiosonde app (design note, not built)

Status 2026-11-12: **proposal.** Nothing here runs yet. What is built is FSK PROBE (launcher slot 8), preset for the
test described under "Step 1", with a capture mode (`#`) and `probe_capture.py` to download it for step 2.

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
