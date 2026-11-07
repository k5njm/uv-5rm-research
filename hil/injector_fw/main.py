# MicroPython for a Raspberry Pi Pico: presses radio keys through optocouplers wired across the
# keypad matrix contacts. UNTESTED ON HARDWARE (written before the rig existed).
#
# keymap.json on the board maps key name -> GPIO number driving that key's optocoupler LED, e.g.
#   {"menu": 2, "exit": 3, "up": 4, "down": 5, "star": 6, "pound": 7, "ab": 8, "0": 9, ... "9": 18}
# SAFETY: there is intentionally no "ptt" entry and the firmware rejects it. The PTT line is never
# wired to the injector. A watchdog releases all keys if the host goes quiet.
import json, sys, time
from machine import Pin

MAX_MS = 5000
WATCHDOG_MS = 8000

keymap = json.load(open("keymap.json"))
assert "ptt" not in keymap, "ptt must never be wired to the injector"
pins = {name: Pin(gpio, Pin.OUT, value=0) for name, gpio in keymap.items()}


def all_up():
    for p in pins.values():
        p.value(0)


def handle(line):
    parts = line.split()
    if not parts:
        return "ERR empty"
    if parts[0] == "PING":
        return "PONG injector-1"
    if parts[0] == "ALLUP":
        all_up()
        return "OK"
    if parts[0] == "KEY" and len(parts) == 3:
        name, ms = parts[1], int(parts[2])
        if name not in pins:
            return "ERR unknown key"
        if not 1 <= ms <= MAX_MS:
            return "ERR ms"
        pins[name].value(1)
        time.sleep_ms(ms)
        pins[name].value(0)
        return "OK"
    return "ERR syntax"


last = time.ticks_ms()
while True:
    # blocking readline is fine: the watchdog below only matters if a press hangs the loop
    line = sys.stdin.readline()
    last = time.ticks_ms()
    try:
        print(handle(line.strip()))
    except Exception as e:  # never leave a key held
        all_up()
        print("ERR", e)
    if time.ticks_diff(time.ticks_ms(), last) > WATCHDOG_MS:
        all_up()
