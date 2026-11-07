"""Pointer and keyboard for a GNOME (mutter) session through org.gnome.Mutter.RemoteDesktop.

    python3 gnome_input.py [monitor connector, default Meta-0 (mutter's virtual monitor)]

Reads commands on stdin and answers each with "ok" or "error ...", like e2e/wlpointer:
  click X Y | scroll X Y STEPS (positive = down) | key NAME[+NAME…] | type TEXT
Mutter only takes absolute pointer positions relative to a screen cast of a monitor, so this
records the monitor too (which needs PipeWire running; run_linux.sh starts it).
"""

import sys
import time

import dbus

KEYSYMS = {"Return": 0xFF0D, "Enter": 0xFF0D, "Escape": 0xFF1B, "Tab": 0xFF09,
           "BackSpace": 0xFF08, "End": 0xFF57, "Home": 0xFF50, "Down": 0xFF54, "Up": 0xFF52,
           "Left": 0xFF51, "Right": 0xFF53, "ctrl": 0xFFE3, "shift": 0xFFE1, "alt": 0xFFE9}
BTN_LEFT = 0x110
RD = "org.gnome.Mutter.RemoteDesktop"
SC = "org.gnome.Mutter.ScreenCast"

bus = dbus.SessionBus()
path = bus.get_object(RD, "/org/gnome/Mutter/RemoteDesktop").CreateSession(dbus_interface=RD)
rd_session = bus.get_object(RD, path)
session = dbus.Interface(rd_session, RD + ".Session")
session_id = rd_session.Get(RD + ".Session", "SessionId", dbus_interface=dbus.PROPERTIES_IFACE)
sc_path = bus.get_object(SC, "/org/gnome/Mutter/ScreenCast").CreateSession(
    {"remote-desktop-session-id": session_id}, dbus_interface=SC)
monitor = sys.argv[1] if len(sys.argv) > 1 else "Meta-0"
stream = bus.get_object(SC, sc_path).RecordMonitor(monitor, {}, dbus_interface=SC + ".Session")
session.Start()


def keysym(name):
    if name in KEYSYMS:
        return KEYSYMS[name]
    if len(name) == 1:
        return ord(name) if ord(name) < 0x100 else 0x1000000 | ord(name)
    raise ValueError(f"unknown key {name}")


def press(sym, down):
    session.NotifyKeyboardKeysym(dbus.UInt32(sym), down)


def move(x, y):
    session.NotifyPointerMotionAbsolute(stream, float(x), float(y))
    time.sleep(0.05)


def handle(line):
    cmd, _, rest = line.partition(" ")
    if cmd == "click":
        x, y = map(int, rest.split())
        move(x, y)
        session.NotifyPointerButton(BTN_LEFT, True)
        time.sleep(0.03)
        session.NotifyPointerButton(BTN_LEFT, False)
    elif cmd == "scroll":
        x, y, steps = map(int, rest.split())
        move(x, y)
        for _ in range(abs(steps)):
            session.NotifyPointerAxisDiscrete(dbus.UInt32(0), dbus.Int32(1 if steps > 0 else -1))
            time.sleep(0.03)
    elif cmd == "key":
        *mods, key = rest.split("+")
        for m in mods:
            press(keysym(m), True)
        press(keysym(key), True)
        press(keysym(key), False)
        for m in reversed(mods):
            press(keysym(m), False)
    elif cmd == "type":
        for ch in rest:
            press(keysym(ch), True)
            press(keysym(ch), False)
            time.sleep(0.02)
    else:
        raise ValueError(f"unknown command {cmd}")


# The seat only gets a pointer with the first event, and that event is lost; send it now.
time.sleep(0.5)
move(1, 1)
time.sleep(0.5)
print("ready", flush=True)
for line in sys.stdin:
    line = line.rstrip("\n")
    if not line:
        continue
    try:
        handle(line)
        print("ok", flush=True)
    except Exception as e:
        print(f"error {e}", flush=True)
