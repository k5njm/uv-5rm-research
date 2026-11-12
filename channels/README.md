# channels/

CHIRP-format CSV channel lists that `firmware/source-changes/channel_plan.py` turns into a channel image for the radio.
Status: the channel path (`channel_plan.py`, then `restore_backup.py`) has loaded 115 and then 116 channels onto the owner's radio and read all 1,000
slots back through the port (AUDIT 32, 37, 40). The details table (`channel_extras.py`) has been written and read back on the radio too. The FM station
list (`fm_stations.py`) and the installer (`install.py`, which runs all of these) have only been tried against the fake radio. The easiest way to load
everything is the installer: [docs/INSTALL.md](../docs/INSTALL.md); what each tool replaces is in the
[user guide](../docs/USER-GUIDE.md#loading-from-a-computer).

- One file per channel group. The file name must match a group name or short name from
  `firmware/firmware-source/src/app/group_table.rs` (case-insensitive, `-`/`_` count as spaces): `repeaters.csv`, `weather.csv`,
  `frs.csv`, `pd-fd.csv`, ... Channels fill that group's slots in file order. The `Location` column is ignored.
  `all.csv` is the exception: channels go to their own `Location` (1-1000).
- Groups overlap, so each file uses only its group's own slots: weather 1-7, repeaters 234-304, frs 305-326,
  gmrs 327-344, murs 345-349, simplex 351-380, satellites 381-410, sonde 411-420 (`sonde.csv`: the Fort Worth radiosonde, receive only).
- `weather.csv`, `frs.csv`, `murs.csv` and `simplex.csv` (national calling frequencies and the 2 m simplex channels) are
  the same everywhere in the US. `repeaters.csv` (ham) and `gmrs.csv` (GMRS repeaters, with their tones) are local to
  the Dallas area. `satellites.csv` holds the FM satellites a handheld can work, from AMSAT's FM satellite summary
  (2026-10-31): check AMSAT's status page before a pass, because satellites come and go. Each cross-band satellite has
  five channels, AOS to LOS, stepping the 70 cm frequency for Doppler; `Duplex` is `split` and `Offset` is the uplink.
- This radio is not certified for FRS, GMRS or MURS: those lists are for listening unless you know your licence and the
  rules allow it; the radio's TX BANDS and TX INHIBIT settings decide what it will transmit on.
- `Detail` (location or description, at most 16 ASCII characters) goes into a second file, `OUT.extras`, which the main
  screen shows beside the channel number in memory mode. Install it with `channel_extras.py PORT build-out/channels.extras`.
- **FM broadcast station lists are not memory channels** and do not go here: `channel_plan.py` reads every `.csv` in this
  directory as a channel group, so a file such as `fm-broadcast.csv` fails the build (`no group is named ...`). They live in
  `channels/fm/` (a subdirectory, which `channel_plan.py` does not read), one file per list, and go to the FM app through
  `fm_stations.py`, a separate path described below.
- Required columns: `Name`, `Frequency`; the rest of the CHIRP columns are optional (see the docstring for the rules).
- Names are at most 12 printable ASCII characters. They are never truncated; a longer name fails the build.
- A build fails, and writes nothing, if a group overflows, two files claim a slot, or a row cannot be parsed.

```
python3 firmware/source-changes/channel_plan.py build channels -o build-out/channels.img [--base my-backup/NAME.img]
python3 firmware/source-changes/restore_backup.py PORT build-out/channels.img --before build-out/undo.img            # dry run
python3 firmware/source-changes/restore_backup.py PORT build-out/channels.img --before build-out/undo.img --write    # writes
```

(`scripts/dev.sh channels` runs the build step in the container.) `channel_plan.py show IMAGE` prints any image or backup.

Without `--base`, every slot the CSVs do not fill is written EMPTY, erasing whatever the radio has there (the undo file
restores it). Use `--base` with a `radio_read.py` backup to keep the other channels. `restore_backup.py` rewrites **whole 4 KB sectors** (128 channels): the
firmware erases a channel's sector the first time a session writes into it (AUDIT 37), so it sends every channel the image holds in any sector it touches.
The channel details table is written whole too, from the lists you build: the details of channels that are not in those lists are lost (the channels stay). Channels whose transmit frequency is
outside the radio's transmit plan are flagged in the table but still written, as the upstream uploader does.

## FM station names (`channels/fm/`)

`channels/fm/dallas.csv` is a list of Dallas-area broadcast stations (from radio-locator.com, receive only) for the FM app;
copy it and put in your own. CHIRP format, but only three columns are read: `Name` (at most 12 printable ASCII characters,
not truncated), `Frequency` (MHz, 87.5 to 108.0, a multiple of 0.1) and `Detail` (at most 16 characters, shown under the
name: a format or a city). Other columns are ignored, so a CHIRP export works as it is. At most 127 stations, no frequency
twice. The name need not repeat the frequency: the app shows both. Nothing about Dallas is in the app.

```
python3 firmware/source-changes/fm_stations.py build channels/fm/dallas.csv -o build-out/fm.fmst
python3 firmware/source-changes/fm_stations.py install PORT build-out/fm.fmst --before build-out/fm-before.bin
```

The table goes to 0x1D8000 of the radio's data flash (two 4 KB halves; the FM app keeps its saves there) through a
programming-port command of its own (`F`), which erases those two sectors and accepts nothing outside them. That command
is new in the core: an older core refuses it and nothing is written. Installing replaces the list on the radio, including
stations saved there. `fm_stations.py show FILE` prints a table file or the `--before` backup. Like everything here it has
only been tried against the fake radio.
