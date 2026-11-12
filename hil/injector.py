"""Host side of the keypad injector, plus a fake for dry runs.

Wire protocol (ASCII lines, 115200 8N1, see injector_fw/main.py):
    PING            -> PONG injector-1
    KEY <name> <ms> -> OK | ERR <reason>     press <name> for <ms> (1..5000), then release
    ALLUP           -> OK                    release everything
`ptt` is deliberately not a key the injector can press: the rig never keys the transmitter.
"""
import time

MAX_MS = 5000
FORBIDDEN = {"ptt"}


class InjectorError(RuntimeError):
    pass


class FakeInjector:
    """Records presses; used for dry runs and tests."""
    def __init__(self):
        self.log = []

    def press(self, name, ms=120):
        _check(name, ms)
        self.log.append((name, ms))

    def close(self):
        pass


def _check(name, ms):
    if name in FORBIDDEN:
        raise InjectorError(f"refusing to press {name}: the rig never keys the transmitter")
    if not 1 <= ms <= MAX_MS:
        raise InjectorError(f"press time {ms} ms outside 1..{MAX_MS}")


class SerialInjector:
    def __init__(self, port, timeout=2.0):
        import serial  # pyserial, only needed on the real rig
        self.s = serial.Serial(port, 115200, timeout=timeout)
        self._cmd("PING", expect="PONG")
        self._cmd("ALLUP")

    def _cmd(self, line, expect="OK"):
        self.s.write((line + "\n").encode())
        reply = self.s.readline().decode().strip()
        if not reply.startswith(expect):
            raise InjectorError(f"{line!r} -> {reply!r}")
        return reply

    def press(self, name, ms=120):
        _check(name, ms)
        self._cmd(f"KEY {name} {ms}")
        time.sleep(ms / 1000 + 0.05)

    def close(self):
        try:
            self._cmd("ALLUP")
        finally:
            self.s.close()
