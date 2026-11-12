"""Parser for the scenario files shared by the simulator and the hardware rig.

The simulator (firmware/source-changes/simulator/scenario.rs) is the reference; this parses the
same text. Steps the rig cannot do on a real radio are reported, never silently skipped:
`coords`, `step`, `expect backlight`, `expect fmlive` and the radio-condition setup steps (`master`, `signal`, `chan`, `air`, `handoff`, `group`) need the
simulator's access to internals; on a real radio the equivalent conditions (a signal generator, stored channels)
must be arranged by hand before the run, so these are listed in the report, never silently skipped.
"""
from dataclasses import dataclass

KEYS = {"star": 10, "pound": 11, "menu": 12, "exit": 13, "up": 14, "down": 15, "ab": 17,
        **{str(d): d for d in range(10)}}
SIM_ONLY = {"coords", "step", "master", "signal", "chan", "air", "handoff", "group", "fmstation", "fmtable", "fmcore"}


@dataclass
class Step:
    line: int
    op: str
    args: list


def parse(text):
    steps = []
    for n, raw in enumerate(text.splitlines(), 1):
        t = raw.split("#", 1)[0].split()
        if not t:
            continue
        op, args = t[0], t[1:]
        if op in ("key", "hold") and (len(args) != 1 or args[0] not in KEYS):
            raise ValueError(f"line {n}: bad key step {raw.strip()!r}")
        if op == "wait" and (len(args) != 1 or float(args[0]) < 0):
            raise ValueError(f"line {n}: wait needs seconds")
        if op == "shot" and len(args) != 1:
            raise ValueError(f"line {n}: shot needs a name")
        if op not in ("app", "key", "hold", "wait", "shot", "expect", "allow", "coords", "step", "master", "signal", "chan", "air", "handoff", "group", "fmstation", "fmtable", "fmcore"):
            raise ValueError(f"line {n}: unknown step {op!r}")
        steps.append(Step(n, op, args))
    return steps


def uses_tx(steps):
    return any(s.op == "allow" and s.args == ["tx"] for s in steps)


def sim_only(steps):
    return [s for s in steps if s.op in SIM_ONLY or (s.op == "expect" and s.args[:1] in (["backlight"], ["fmlive"], ["fmtable"]))]
