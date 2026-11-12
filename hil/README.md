# Hardware-in-the-loop rig (prepared before the hardware exists)

Goal: let an agent (or you) replay the **same scenario files the simulator runs**
(`firmware/source-changes/simulator/tests/scenarios/*.txt`) on a real radio, and check what the LCD
shows with a webcam. Everything here is tested offline with a synthetic camera and a fake injector
(`python3 hil/test_hil.py`); **none of it has touched real hardware yet.**

```
scenario.txt ──► hil_run.py ──► injector (Pico + optocouplers) ──► radio keypad
                     │                                                 │
                     └──── webcam ◄──── LCD ◄───────────────────────────┘
                       rectify to 160x128, compare with simulator golden
```

## Bill of materials (suggested)
| Part | Why |
|---|---|
| Raspberry Pi Pico (or any MicroPython board) | runs `injector_fw/main.py` |
| 18+ optocouplers (PC817 or similar) or analog switches (e.g. CD4066) | "press" a key by closing its contacts without sharing a ground with the radio |
| USB webcam on a fixed mount, plus a hood against glare | LCD capture |
| The **sacrificial** radio, K-plug cable | flashing / readback (`firmware/tools/verify_release.py`, flash scripts) |
| Dummy load on the antenna port, always | the rig never transmits, but a dummy load makes that a hardware guarantee |

## Safety design
* **The injector has no PTT.** `ptt` is not a key it can press, the firmware asserts it is absent from
  the key map, and the host refuses it. Do not wire the PTT contact to the rig.
* `hil_run.py` refuses scenarios containing `allow tx` unless `--allow-tx` is given.
* The simulator already fails any scenario that keys PTT or calls the keying tone function.
* The Pico firmware releases every key on error; the host sends `ALLUP` on exit.

## Wiring the keys
The keypad is a 4x4 matrix plus 4 directly grounded keys (upstream issue #18). Each optocoupler's
output goes across the same two contacts as one key (find them with a multimeter on a switched-off
radio; the radio's own pull-ups define the polarity). Put the GPIO numbers in `keymap.json` on the
Pico (start from `injector_fw/keymap.example.json`). The pin numbers in the example are placeholders.

## Procedure
1. Flash `injector_fw/main.py` + `keymap.json` to the Pico; check `PING` over serial returns `PONG injector-1`.
2. Mount the webcam; with the radio showing any bright screen, note the four LCD corners in webcam pixels
   and write them to `calibration.json` (see `calibration.example.json`: TL, TR, BR, BL).
3. Open the app under test on the radio by hand.
4. `python3 hil/hil_run.py SCENARIO.txt --golden firmware/source-changes/simulator/tests/golden --calibration hil/calibration.json --injector /dev/ttyACM0 --camera /dev/video0 --assume-open`

Dry run (no hardware): add `--dry-run` instead of the hardware arguments.

## What the webcam check can and cannot tell you
Measured with the synthetic camera (perspective, glare, noise, blur) over every golden frame:
* Over all 73 golden frames of the nine shipped apps and HAM TOOLS: the correct screen scores **>= 0.857**; a screen
  from a *different app* scores **<= 0.65** (the worst pairs are firmware list screens that share chrome, e.g. the
  settings group list vs the channel list). Default threshold 0.80 separates them, with less margin than for
  screens with different layouts.
* Gross state changes are detected (the flashing "ID NOW" banner vs armed: 0.45).
* Colour-only changes are borderline (stopwatch running vs stopped, green vs white digits: 0.91 vs 0.84).
* **Small text changes are not detected** (e.g. `EVERY 10:00` vs `12:00`). This is a limit of the method,
  pinned by a test. Fine text is verified pixel-exactly in the simulator; the rig confirms the real radio
  shows the right screen in the right state. A macro lens, higher-resolution camera or OCR on a cropped
  region could extend this later.
* `coords`, `step`, `expect backlight` and the radio-condition steps `master`, `signal`, `chan`, `handoff` are simulator-only and are
  reported, not silently skipped. On a real radio, arrange the equivalent by hand first (a signal generator or a second
  radio on the right frequency, stored channels) or the golden will not match.
* Scenarios containing `allow tx` (e.g. TEXT) are **refused** by the rig unless `--allow-tx` is passed; the dry-run test pins this.
* Real-time scenarios run in real time (the 10-minute ID timer scenario takes 10 minutes on the rig).
* Menu navigation to open an app is not automated (it varies by radio); use `--assume-open`.
