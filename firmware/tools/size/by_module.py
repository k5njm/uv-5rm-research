import re, subprocess, collections, sys
elf = "/cache/fw/thumbv7em-none-eabi/release/bf5rh-fw"
out = subprocess.run(["rust-nm", "-S", "--size-sort", "-C", elf], capture_output=True, text=True).stdout
syms = []
for line in out.splitlines():
    p = line.split(None, 3)
    if len(p) == 4 and re.fullmatch(r"[0-9a-fA-F]+", p[1]):
        syms.append((int(p[1], 16), p[2], p[3]))
total = sum(s for s, _, _ in syms)
print(f"symbols: {len(syms)}, total sized bytes: {total}")
def bucket(name):
    n = re.sub(r"^<(.*?) as .*?>", r"\1", name)
    n = re.sub(r"^<", "", n)
    parts = n.split("::")
    if parts[0].startswith("bf5rh_fw") and len(parts) > 1:
        return "bf5rh_fw::" + parts[1] + (("::" + parts[2]) if parts[1] in ("ui", "app", "drivers", "device") and len(parts) > 2 else "")
    return parts[0]
agg = collections.Counter()
for s, t, n in syms:
    agg[bucket(n)] += s
print("\n== by module (bytes)")
for k, v in agg.most_common(34):
    print(f"{v:6d}  {k}")
print("\n== 22 largest single symbols")
for s, t, n in sorted(syms, reverse=True)[:22]:
    print(f"{s:6d} {t} {n[:100]}")
