# APRS as a test signal for FSK PROBE (144.390 MHz, receive only)

Status 2026-11-12: **built and tested in the simulator and on the host only. Nothing here has been seen on the radio.** The sync
words, the odds and the outcome table below are derived from documents and arithmetic, not measured. Companion to
[SONDE-APP.md](SONDE-APP.md) (the RS41 plan and the earlier probe tests) and to the register notes in
[HARDWARE.md](../HARDWARE.md) section 6.

## Why APRS, and what it can and cannot show

The probe's modem setup was fixed on 2026-11-12 from Fudan's register document and has **never been seen working**: the only
capture predates the fix. The target is the RS41 radiosonde, but the next launch is a while away. APRS packets are on 144.390 MHz
every few seconds, so they make a stand-in signal today, with a second receiver (the owner's SDR and Direwolf) to say what was
on the air.

The value of the test does not depend on decoding APRS. A capture on 144.390 MHz answers, either way:

* does the corrected setup raise a sync only on a real transmission (against the noise-only baseline in
  [SONDE-APP.md](SONDE-APP.md) (a));
* once a sync has fired, do words stream continuously, how many, and how far apart (the tick timing: 13.3 ms a word at 1200 baud);
* what the chip delivers at the end of a short frame (it keeps clocking the channel's noise until its count runs out);
* whether the delivered bits have any flag structure at all (the host decoder reports this with and without a FCS pass);
* the signal strength and tone readout (REG 6E) during a packet.

## What Bell 202 is, and what this chip offers

APRS is AX.25 over Bell 202 AFSK on FM: 1200 baud, mark 1200 Hz, space 2200 Hz, audio tones. NRZI coded (a 0 bit changes the tone,
a 1 bit keeps it), HDLC framed (flag `7E`, sent least-significant bit first; a 0 stuffed after five 1s; a preamble of repeated
flags, TXDELAY, typically 200 to 500 ms), CRC-16/X.25 as the frame check, low byte first. There is **no fixed sync word** other
than the flags.

The FD6818B register document (Fudan, 2025/1/11, page 10, read from the rendered page) offers for REG_58:

| REG_58 field | Values (document) |
|---|---|
| bits 12:10, FSK RX mode | `000` FSK 1.2K, FSK 2.4K **and NOAA SAME**; `111` FFSK 1200/1800; `100` **FFSK 1200/2400** |
| bits 3:1, FSK RX bandwidth | `100` FSK 2.4K **and FFSK 1200/2400**; `000` FSK 1.2K; `001` FFSK 1200/1800; `010` NOAA SAME |
| bits 15:13, TX mode (not used: this app is receive only) | `000` FSK 1.2K/2.4K, `001` FFSK 1200/1800, `011` FFSK 1200/2400, `101` NOAA SAME |

HARDWARE.md's earlier table listed only 1200/1800 and left the mode field at 000; the document has a second FFSK pair, **1200/2400**,
and it is the closer one: its mark tone is Bell 202's mark exactly, its space tone is 2400 against 2200 (200 Hz, 8% off), while
1200/1800 is 400 Hz off. There is no 1200/2200 mode. So the probe's first try is FFSK 1200/2400 (`F1224`), then 1200/1800
(`F1218`), then plain FSK.

### What is known about receiving Bell 202 on this chip family (searched 2026-11-12)

* **Nothing positive was found.** No project was found that receives APRS with a BK4819 or FD6818 modem. Everything below is
  discussion, not a result.
* UV-K5 custom firmware thread, [piotr022/UV_K5_playground issue 20](https://github.com/piotr022/UV_K5_playground/issues/20)
  (July to August 2023, KISS TNC / APRS / BBS over a BK4819): one participant wrote "after testing I have confirmed that BK4819
  produces FSK, not AFSK" and concluded it would not be useful for APRS (no method or data shown); another pointed out that the
  register map lists `FFSK1200/2400` and that APRS needs NRZI "in software"; a third suggested both sync bytes `0x7E` to trigger
  on packets and 1200 baud with 1200/2200 Hz tones, "untested"; and that transmit looks easy but "reception might be more
  difficult". No result was reported.
* The BK4819 firmware forks that have an FSK receive path use it for fixed-length messages (AIRCOPY) and, in some forks, for
  MDC1200 (1200 baud, 1200/1800 Hz). I could not read their register values here, so I do not cite them for the mode bits.
* An older vendor programming guide for the FD6818 (REV 1.0.0, found as an unofficial copy on mikrocontroller.net,
  [attachment 608473](https://www.mikrocontroller.net/attachment/608473/Confidential_Programming_guide_FD6818.pdf), pages 36-37)
  describes two frame formats for the FSK modem: format 1, a fixed length with an optional chip CRC, and format 2, "unlimited
  reception", where the device "permanently" imports the data after the sync word into the FIFO. The transmit side writes a **type
  byte** (`0x01` = format 2, `0x21` = format 1 without CRC, `0x61` = with CRC) and a length in words (8 bits, "(n+1) words"). It
  does not say how a receiver picks the format. That guide is for the older chip: 64-word FIFOs and a length in words, where
  the FD6818B register document has an 8-word RX FIFO and a length in bytes (11 bits). **Open question, unverified:** whether
  the FD6818B receiver treats the first byte after the sync as such a type byte. If it does, an APRS or RS41 stream has no
  type byte, and the first delivered byte would be misread as one. The capture shows what the first words are either way.
* Nothing in the document says whether the FFSK demodulator recovers a bit clock from a preamble or just samples on its own
  baud clock from the sync match.

### The odds, plainly

These are my estimates, not measurements.

| Outcome | Estimate |
|---|---|
| The capture teaches us something about syncs, streaming and timing (any outcome except "the radio did not run") | about 95% |
| The modem raises syncs on real packets in at least one mode | about 35% |
| The words then contain flag structure in at least one reading (the host decoder sees runs of flags) | about 30% |
| At least one frame passes its FCS | about 15% |
| It decodes most of what Direwolf does | under 3% |

Why not lower: a per-bit tone detector tuned to 1200/2400 sees Bell 202's 2200 Hz space at about 95% correlation after one bit
time (200 Hz off, a bit is 1/1200 s), and a frequency discriminator with its threshold between the two tones (1800 Hz for
1200/2400) puts 1200 and 2200 on opposite sides. Why not higher: nobody has reported it working, "FSK not AFSK" is the one
test anyone claims, and the chip's bit clock must stay in step with a transmitter it never locked to (a 1% baud error slips a bit
every 100 bits; a frame of 500 bits would slip five times). The crystal is also unverified: the baud register assumes 13 or 26 MHz.

## Sync words for the HDLC flag preamble (derivation)

A flag is `0x7E` = `01111110`, sent least-significant bit first: bits `0 1 1 1 1 1 1 0` (six 1s between two 0s). NRZI: a 0 changes the
tone, a 1 keeps it. Let `a` be the tone before the flag (mark or space). The first 0 changes it to `!a`; the six 1s keep `!a`; the
last 0 changes it back to `a`. So a flag is **seven bit times of `!a`, then one of `a`**, and it ends on `a`, where the next flag
starts. Every flag in a preamble is identical: a pattern with a period of 8 bits and **no second phase after the first flag**. The two
cases are the two assignments of tones to the demodulator's output bit:

| Tone before the preamble | Raw bits in time order | As bytes, first bit in time = most-significant | As bytes, first bit in time = least-significant |
|---|---|---|---|
| `a` = 0 | `1111 1110` repeating | `FE` | `7F` |
| `a` = 1 | `0000 0001` repeating | `01` | `80` |

The sync registers hold four bytes, sync byte 0 first (REG_5A high byte first), so the 32-bit presets are `FEFE FEFE`,
`0101 0101`, `7F7F 7F7F` and `8080 8080`; with the 2-byte sync (S2) only `FEFE`, `0101`, `7F7F` or `8080` is compared. Key `5` steps
through them in that order. Which bit order the chip uses within a byte is unverified (it is also the open question for the RS41
presets, keys `8` and `9`). The preset the `APRS` key uses is `FEFE FEFE`: mark taken as 1 and the document's "most-significant bit
first" reading. Because the pattern repeats every 8 bits, any of its rotations also matches somewhere in a preamble. The chip
fires on the first alignment it finds, and the host decoder works on the bit stream, so which rotation fired does not matter.
A modem that preambles with zero bytes instead of flags sends alternating tones; keys `6` (`AAAA`, `5555`) cover that.

**False syncs on noise.** A 32-bit match on random bits is 2^-32 per bit position; at 1200 positions a second that is one false
sync in about 41 days. A 16-bit match (S2) is 2^-16: 1200 / 65,536 = 0.018 a second, one in about 55 s (FM noise is correlated, so
more often; the first capture's 16-bit match on noise gave one event in 2.9 s at 4800 baud where the arithmetic gives 0.2: one
event, not a measurement). So S4 should be silent on noise and fire on a real preamble that has four clean flag bytes (the first few
bytes after a transmitter keys up are often ragged); S2 fires on noise about once a minute and on half a preamble.

## What the APRS key sets

On FSK PROBE, with the option cursor on `PRE` (where it starts), key `0` switches between the two presets in one press:

| | RS41 | APRS |
|---|---|---|
| Frequency | 404.800 MHz | 144.390 MHz |
| Baud (register) | 4800 (`C195`) | 1200 (`3065`) |
| RX mode (REG_58 12:10) and bandwidth (3:1) | FSK, 2.4K (`0309`) | FFSK 1200/2400, its own 2.4K (`1309`) |
| Sync | `086D 5388`, 4 bytes | `FEFE FEFE`, 4 bytes |
| Chip length (REG_5D), bytes after the sync | 316 (`3B20`) | 256 (`FF00`) |
| App's own frame, words | 158 | 128 |
| Threshold | 1 word per interrupt | 1 word per interrupt |

The window is 256 bytes (1.71 s at 1200 baud, 128 words): the largest the verified 8-bit length field can say, so it does not rely
on the unverified top 3 bits. The sync fires on the first clean 32 bits of the preamble, so the window holds the rest of the
preamble (TXDELAY 300 ms is 45 flag bytes, of which the sync used 4) and then the frame: **frames up to about 200 bytes fit whole**;
a longer one is cut off (the decoder reports a missing closing flag). AX.25 frames in APRS are 30 to 330 bytes, mostly under 100.
**At the end of a short frame** the transmitter stops, and the chip goes on clocking whatever the demodulator produces (noise) as
words until its count runs out: a 60-byte packet leaves about 70 words of noise, and a second packet that starts inside that
window (a digipeater repeating it a second later) is missed, because the chip is not listening for a sync until the window ends.
The app then re-arms. The decoder treats the noise after the closing flag as noise.

The `MODE` item (`A/B` to it, then `0`) steps FSK, `F1224` (FFSK 1200/2400), `F1218` (FFSK 1200/1800), each with the bandwidth the
document pairs with it; `BW` (`AUTO`, `1.2`, `2.4`, `1218`) forces a neighbouring bandwidth. A hand change turns the preset
name on the screen to `CUST`. Every setting is written into the capture's header (version 3); `probe_capture.py` prints them.

## The reference receiver: Direwolf on the Mac, with the RTL-SDR

Receive only: this configuration has no transmitter, no `PTT` line, no beacon and no internet gateway, so Direwolf sends nothing.
Install (Homebrew; skip what is already there):

```
brew install librtlsdr direwolf
rtl_test -t                   # finds the dongle; Ctrl-C when it says so
```

Save this as `sdr.conf` (made-up call sign; it is only used if Direwolf were told to send, which this file does not):

```
ADEVICE stdin null
ACHANNELS 1
CHANNEL 0
MYCALL N0CALL
MODEM 1200
```

Run (the antenna of the SDR within a few metres of the radio's; adjust `-g` gain and `-p` ppm for your dongle):

```
rtl_fm -M fm -f 144.390M -s 24000 -g 40 -l 0 - | direwolf -c sdr.conf -r 24000 -t 0 -T "%Y-%m-%d %H:%M:%S" -l direwolf-logs - | tee aprs-reference.txt
```

`-T` puts a wall-clock timestamp in front of every frame Direwolf decodes, `tee` keeps them in `aprs-reference.txt`, and `-l direwolf-logs`
makes Direwolf write its own CSV log (a file per day, with the time of each packet) in that directory. Watch it for a minute before
touching the radio: decoded lines like `[0.4] N0CALL>APRS,WIDE1-1:...` should appear every few seconds. If none do, move the
antenna or raise `-g` before suspecting the radio. Set the Mac's clock from the network (System Settings, Date & Time).

## The test (about 20 minutes, all receive only)

1. Flash the image built by `scripts/dev.sh release` whose sha256 equals the pin in `firmware/expected-hashes.json`.
   Start the reference receiver above and check it decodes.
2. Open FSK PROBE (`MENU`, `8`). Press `0` once (the cursor is on `PRE`): the title row shows `144.390 1200 baud`, the sync row
   `SYNC FEFE`, the option row `APRS F1224 LEN256 TH1` and `INV- BWAUTO S4`.
3. Press `MENU` (modem on). Do not record yet. Watch `SYN` and `WRD` for two minutes while the reference decodes packets, and
   write them down with the number of packets the reference decoded. Press `UP` once from REG 02, or `DOWN` sixteen times, to
   reach **REG 6E**: it shows the audio tone the chip measures in Hz (REG 6E = Nout x 25390.625 / Rout, document page 11). Note
   what it reads while a packet is on the air: 1200 and 2200 mean the signal reaches the modem input; 0 or a constant means it
   does not.
4. Press `#`, and at the same moment read the Mac's clock (`date +%T`) and write it down. Wait **3 minutes**. The capture holds
   **16,352 records**: at 1200 baud a word is 13.3 ms, so continuous streaming would fill it in 16,352 x 13.3 ms = **218 s**
   (3 min 38 s; 216 s with the sync records). It is not continuous: the chip only streams for 128 words (1.7 s) after a sync, so
   in practice the limit is 126 sync windows, which on a busy frequency is many minutes. Three minutes is 10 to 40 packets
   and never full unless something syncs constantly (then the screen says `REC FULL`, which is itself a finding).
5. Press `#` (`SAVED n`), leave the app (`EXIT`) and, with the radio on its main screen, on the computer:

   ```
   python3 firmware/source-changes/probe_capture.py PORT aprs-a.bin
   ```

   It prints the usual summary, then the AX.25 decode (it runs by itself for a 1200 baud capture; add `--ax25` to force it on
   anything). Each decoded frame is printed as `SOURCE>DEST,PATH:payload` with its time in seconds after the capture started:
   add that to the clock time you wrote down and look it up in `aprs-reference.txt`. To decode again later:
   `probe_capture.py --decode aprs-a.bin --ax25`.
6. Repeat steps 4 and 5 for each variation you need, one change at a time (the capture header says which). With the
   cursor on `PRE`, `A/B` once moves it to `MODE` and `0` steps `F1224`, `F1218`, `FSK`, `F1224`; `A/B` again moves it on to
   `LEN`, `TH`, `INV`, `BW` and `S`. In order of usefulness: `F1218`, then `FSK`; then `F1224`
   again with `S2` (five `A/B` from `MODE`, then `0`), which also syncs on noise and on half a preamble, so words stream even if
   the full preamble never matches; then `INV+`; the other flag patterns (key `5` steps `FEFE`, `0101`, `7F7F`, `8080`); and `BW`
   forced to `1.2` and `1218`.

## What each result means, and what to try next

| Result | What it means | Next |
|---|---|---|
| **No syncs at all** (`SYN` stays 0, summary: no records) while the reference decodes packets and RSSI shows a signal | The modem is not matching the flag pattern in the bits it makes. Either it is not hearing the tones (wrong tone pair, bandwidth), or its bits differ from the derivation (bit order, polarity), or the sync compare needs something else (the older guide's type byte, preamble length). REG 6E not showing 1200/2200 would point to the input, not the sync | Another mode (`F1218`, then `FSK` with `BW` at both settings); the other flag patterns (key `5`: four presses); `S2` (16 bits, fires on a half preamble and on noise: if even S2 never fires on a signal, the modem is not seeing the signal as bits at all); `INV+`; key `6` (alternating preamble) in case the sender preambles with zeros. Then the noise baseline in SONDE-APP (a) to check the probe still syncs on noise |
| **Syncs but no words** (`SYN` rising, `WRD` 0; frames show a sync and a done flag only) | The first capture's signature again: the FIFO is not filling after a sync. The setup fixes (clear bit released, 4-byte sync) did not cure it, or the modem needs the type byte | `TH4`; `LEN 256`; compare the flags in the CSV (`done` with no words); read REG 59 and REG 5D back with `UP`/`DOWN` (they should read `1008` and `FF00`); then SONDE-APP (a) step 4 |
| **Words but no flag structure** (the decoder says: no flag in any reading; about 50% ones; run lengths halve each step; 8+ runs about 0.8%) | The modem streams, but the bits are noise as far as AX.25 goes: it synced on something and carries on, or it is not hearing the signal | Compare the words with the reference's timestamps (do syncs line up with packets? if not, they are false syncs); another mode and bandwidth; `S4` against `S2`; check the ones fraction while the radio hears a strong packet vs. none |
| **Flag structure, no good frame** (flags found, longest run of flags near the preamble length, frames tried, bad FCS) | The preamble comes through, so the modem hears the signal and the bit order is likely right (the table says which reading found the flags). The body is wrong: bit errors, a slipped clock or a short window | Look at "one flipped bit": if single flips fix frames, errors are sparse (try the neighbouring `BW`/mode; a higher `RSSI`); if the address bytes look right but the FCS fails everywhere, bits are slipping (compare "average bits/s delivered" against 1200: 1% off slips a bit every 100; it means the baud register or crystal is off); frames cut off at the window mean a longer one is needed (`LEN 2048` ends the frame by the app's count, so it is not shorter) |
| **Frames decoded** | The chip can receive APRS in the mode the header names. | Count decoded against the reference, per mode and bandwidth; test the weakest packets; REG_73 (AFC) is not on the app's allow-list, so no AFC test is possible without a core change; then the same machinery should serve RS41 and Horus (SONDE-APP) |

What the capture can show **whatever the decoder says**: `time between words` against 13.3 ms (is the tick keeping up: gaps over 30
ms mean it is not), `most words read in one interrupt` (8 means the FIFO may have overflowed), `frames ended by count / by the
chip's done flag / cut off` (does the chip's done come at 256 bytes), the `first words` (does a sync start with the flag pattern
`FEFE`/`0101`, which would settle the polarity and bit order, or with something else), the RSSI during the words, and in the
CSV, whether syncs line up with the reference's packet times.

## What is a guess

* The simulator models the RX mode by "a signal of one tone pair is only heard in the matching mode, and any other mode hears
  nothing". A real chip may half-hear a neighbouring pair or deliver inverted or partial bits. The scenarios for the presets and
  the `MODE` item only show that the registers are written and the counters follow; they say nothing about the chip.
* Which bit and byte order the chip uses: the decoder tries all four, with NRZI undone, as data, and as inverted data.
  NRZI is blind to polarity (inverting every raw bit gives the same data), so there is nothing to choose there.
* The REG 6E formula and units, the crystal (13/26 MHz) and everything in HARDWARE.md section 6 marked unverified.
* The older guide's type byte and frame formats: it describes a different chip revision.
