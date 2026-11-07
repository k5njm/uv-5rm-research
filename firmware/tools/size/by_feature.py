import re, subprocess, collections
elf = "/cache/fw/thumbv7em-none-eabi/release/bf5rh-fw"
print(subprocess.run(["rust-size", "-A", elf], capture_output=True, text=True).stdout.split("\n\n")[0][:900])
out = subprocess.run(["rust-nm", "-S", "-C", elf], capture_output=True, text=True).stdout
syms = []
for line in out.splitlines():
    p = line.split(None, 3)
    if len(p) == 4 and re.fullmatch(r"[0-9a-fA-F]+", p[1]) and p[2] in "tTrRdD":
        syms.append((int(p[1], 16), p[2], p[3]))
def total(pred): return sum(s for s, t, n in syms if pred(n))
api = [(s, n) for s, t, n in syms if "app::overlay::api_" in n]
print(f"\nAPI wrappers: {len(api)} functions, {sum(s for s,_ in api)} bytes (avg {sum(s for s,_ in api)//max(1,len(api))} B)")
print("largest API wrappers:", ", ".join(f"{n.split('api_')[-1]}={s}" for s, n in sorted(api, reverse=True)[:12]))
fam = lambda pat: total(lambda n: re.search(pat, n))
print("\nby feature (code+rodata symbols that carry a name):")
for label, pat in [("FM radio (api_fm_*, rda5807, fm_radio, board::fm)", r"api_fm_|rda5807|fm_radio|board::fm|fm_"),
                   ("satellite/UTC/sgp4", r"sat|sgp4|api_utc|utc_"),
                   ("core::fmt (Formatter, Display, pad_integral...)", r"core::fmt|fmt::num|fmt::builders"),
                   ("core::str / utf8", r"core::str::|from_utf8"),
                   ("embedded_graphics (text drawing, fonts code)", r"embedded_graphics"),
                   ("scan + modes + side keys (app::scan/modes/side*, ui::scan)", r"app::scan|app::modes|app::side|ui::scan|keyfn|convert"),
                   ("DTMF/ANI", r"dtmf|ani|DTMF"),
                   ("settings parse/serialise (Settings::from_bytes/to_bytes, apply_settings)", r"Settings>::(from|to)_bytes|apply_settings"),
                   ("CPS (programming port)", r"bf5rh_fw::cps"),
                   ("storage + wear-levelling", r"device::storage|wear_leveled"),
                   ("memcpy/memset/compiler_builtins", r"compiler_builtins|memcpy|memset")]:
    print(f"  {fam(pat):6d}  {label}")
