"""Replace the start-up tune in the radio's settings, leaving every other setting untouched.

  set_boot_tune.py COM3 BACKUP.bin [doom|mario]
Backs up the 168-byte settings record first, then reads it back and confirms only the tune changed.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

SETTINGS_ADDR, SETTINGS_BYTES, TUNE_OFFSET, TUNE_PAIRS = 0x9000, 168, 72, 48
READ, WRITE = 0x52, 0x57

# Indexes into the firmware's equal-tempered note table. Index 1 is A2 and
# every increment is one semitone; 0 is a rest. Durations are in 30 ms units.
E4, F4, FS4, G4, AB4, A4, BB4, B4 = 20, 21, 22, 23, 24, 25, 26, 27
C5, D5, EB5, F5, G5 = 28, 30, 31, 33, 35
BB5, C6, D6, EB6, E5 = 38, 40, 42, 43, 32

doom_riff = [E4, E4, E5, E4, E4, D5, E4, E4, C5, E4, E4, BB4, E4, E4, B4, C5]
doom = []
for note in doom_riff[:-1]:
    doom += [(note, 4), (0, 1)]
doom.append((doom_riff[-1], 6))

# Opening bar and a half from the existing set_radio2_mario_tune.py. Sounded
# eighth notes use four units plus a one-unit articulation gap; written rests
# use all five units, and the last G4 is held for eight units.
mario_melody = [E5, E5, 0, E5, 0, C5, E5, 0, G5, 0, 0, 0, G4]
mario = []
for note in mario_melody[:-1]:
    if note == 0:
        mario.append((0, 5))
    else:
        mario += [(note, 4), (0, 1)]
mario.append((mario_melody[-1], 8))

presets = {
    "doom": doom,
    "mario": mario,
}
preset = sys.argv[3].lower() if len(sys.argv) > 3 else "doom"
if preset not in presets:
    raise SystemExit("unknown tune: " + preset + " (choose doom or mario)")
tune = presets[preset]
assert len(tune) <= TUNE_PAIRS and all(0 <= n <= 45 and d > 0 for n, d in tune)
tune_bytes = bytes(b for pair in tune for b in pair).ljust(TUNE_PAIRS * 2, b"\0")
millis = sum(d for _, d in tune) * 30
print(f"tune: {preset}, {len(tune)} steps, {millis / 1000:.2f} seconds")

port, backup = sys.argv[1:3]
cps = Cps(port)
before = cps.send(READ, SETTINGS_ADDR)[2]
assert len(before) == SETTINGS_BYTES, len(before)
open(backup, "wb").write(before)
print(f"settings backed up to {os.path.basename(backup)}")
print("boot sound on:", before[31] != 0)

after = before[:TUNE_OFFSET] + tune_bytes + before[TUNE_OFFSET + len(tune_bytes):]
assert len(after) == SETTINGS_BYTES
cps.send(WRITE, SETTINGS_ADDR, after)

check = cps.send(READ, SETTINGS_ADDR)[2]
if check != after:
    print("READ-BACK DIFFERS; restoring the backup")
    cps.send(WRITE, SETTINGS_ADDR, before)
    cps.end()
    raise SystemExit(1)
same = check[:TUNE_OFFSET] == before[:TUNE_OFFSET] and check[TUNE_OFFSET + len(tune_bytes):] == before[TUNE_OFFSET + len(tune_bytes):]
print("tune written and verified" if same else "UNEXPECTED: other settings changed")
print("other settings unchanged" if same else "")
cps.end()
print("done; radio restarting")
