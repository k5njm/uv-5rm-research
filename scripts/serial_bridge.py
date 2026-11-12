"""Serial <-> TCP bridge so a container on a Mac can use the host's USB serial port (containers cannot see it).

  serial_bridge.py PORT TCP_PORT

Connects OUT to 127.0.0.1:TCP_PORT (a loopback-only port the container publishes); nothing listens on the host.
Passes bytes both ways unchanged at 115200 baud, for one connection, and prints the byte counts when it closes.
It is a pipe: what reaches the radio is decided by the program in the container (scripts/chirp-backup.sh only downloads).
"""
import socket, sys, threading, time, serial
port, tcp = sys.argv[1], int(sys.argv[2])
for _ in range(60):
    try:
        c = socket.create_connection(("127.0.0.1", tcp), timeout=2); break
    except OSError: time.sleep(0.5)
else: raise SystemExit("container port never came up")
c.settimeout(None); c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
s = serial.Serial(port, 115200, timeout=0.02, rtscts=False, xonxoff=False)
tx = rx = 0
def up():
    global rx
    while True:
        try:
            d = s.read(256)
            if d: c.sendall(d); rx += len(d)
        except Exception: return
threading.Thread(target=up, daemon=True).start()
try:
    while True:
        d = c.recv(4096)
        if not d: break
        s.write(d); tx += len(d)
finally:
    print(f"bridge closed: {tx} bytes to radio, {rx} bytes from radio"); s.close()
