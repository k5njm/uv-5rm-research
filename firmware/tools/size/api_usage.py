import re, subprocess, glob, collections
fields = re.findall(r"pub (\w+): (?:extern \"C\" fn|extern \"C\" fn)", open("/work/firmware/firmware-source/abi/src/lib.rs").read())
nm = subprocess.run(["rust-nm", "-S", "-C", "/cache/fw/thumbv7em-none-eabi/release/bf5rh-fw"], capture_output=True, text=True).stdout
size = {}
for l in nm.splitlines():
    m = re.search(r"app::overlay::api_(\w+)$", l)
    if m: size[m.group(1)] = int(l.split()[1], 16)
srcs = glob.glob("/work/firmware/firmware-source/apps/**/*.rs", recursive=True)
use = collections.defaultdict(set)
for f in srcs:
    if "/hamtools/" in f and False: pass
    app = re.search(r"apps/([^/]+(?:/[^/]+)?)/", f).group(1)
    app = app.split("/")[0] if app.startswith("settings") else app
    t = open(f).read()
    for fld in fields:
        if re.search(r"\(api\.%s\)|\bapi\.%s\b|\(\*\w+\)\.%s\b|\.%s\)\(" % ((fld,)*4), t):
            use[fld].add(app)
rows = [(size.get(f, 0), f, sorted(use[f])) for f in fields]
print(f"{len(fields)} ABI functions; wrappers sized {sum(r[0] for r in rows)} B in core")
print("\nUNUSED by every app in the repo:")
for s, f, u in sorted(rows, reverse=True):
    if not u: print(f"  {s:4d} B  {f}")
print("\nUSED BY EXACTLY ONE APP (candidates to move into that app, if the hardware access is possible there):")
for s, f, u in sorted(rows, reverse=True):
    if len(u) == 1 and s >= 60: print(f"  {s:4d} B  {f:22} -> {u[0]}")
