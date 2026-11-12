# channels/

CHIRP-format CSV channel lists that `firmware/source-changes/channel_plan.py` turns into a channel image for the radio.
Nothing here has been written to a real radio yet; the whole path is tested only against the fake radio.

- One file per channel group. The file name must match a group name or short name from
  `firmware/firmware-source/src/app/group_table.rs` (case-insensitive, `-`/`_` count as spaces): `repeaters.csv`, `weather.csv`,
  `frs.csv`, `pd-fd.csv`, ... Channels fill that group's slots in file order. The `Location` column is ignored.
  `all.csv` is the exception: channels go to their own `Location` (1-1000).
- Groups overlap, so each file uses only its group's own slots: weather 1-7, repeaters 234-304, frs 305-326,
  gmrs 327-344, murs 345-349, simplex 351-380, satellites 381-410.
- `weather.csv`, `frs.csv`, `murs.csv` and `simplex.csv` (national calling frequencies and the 2 m simplex channels) are
  the same everywhere in the US. `repeaters.csv` (ham) and `gmrs.csv` (GMRS repeaters, with their tones) are local to
  the Dallas area. `satellites.csv` holds the FM satellites a handheld can work, from AMSAT's FM satellite summary
  (2026-10-31): check AMSAT's status page before a pass, because satellites come and go. Each cross-band satellite has
  five channels, AOS to LOS, stepping the 70 cm frequency for Doppler; `Duplex` is `split` and `Offset` is the uplink.
- This radio is not certified for FRS, GMRS or MURS: those lists are for listening unless you know your licence and the
  rules allow it; the radio's TX BANDS and TX INHIBIT settings decide what it will transmit on.
- `Detail` (location or description, at most 16 ASCII characters) goes into a second file, `OUT.extras`, which the main
  screen shows beside the channel number in memory mode. Install it with `channel_extras.py PORT build-out/channels.extras`.
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
restores it). Use `--base` with a `radio_read.py` backup to keep the other channels. Channels whose transmit frequency is
outside the radio's transmit plan are flagged in the table but still written, as the upstream uploader does.
