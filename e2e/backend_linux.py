"""Linux backend for driver.py: AT-SPI (pyatspi) on the app's accesskit tree.

Sessions (set up by run_linux.sh):
  x11    Xvfb; input through xdotool, screenshots with ImageMagick `import`, clipboard with xclip.
  sway   headless sway; pointer through e2e/wlpointer (wlroots virtual pointer), keys through
         wtype, screenshots with grim, clipboard with wl-paste.
  gnome  headless GNOME Shell; pointer and keys through Mutter's RemoteDesktop API
         (e2e/gnome_input.py), clipboard through Xwayland. GNOME doesn't let ordinary clients
         take screenshots, so there are none.
  kde    headless KWin; pointer and keys through KWin's fake input (e2e/wlpointer), screenshots
         through KWin's ScreenShot2 API, clipboard with wl-paste.

Input goes through accessibility actions where possible. The file picker is the real portal
dialog (see dialog.py), except on x11 where fake_portal.py answers.
"""

import json
import os
import subprocess
import time
from pathlib import Path

import pyatspi

import dialog

TITLE = "Frontier Gaming Network"
APP_ID = "network.frontierwinggaming.app"
ROLES = {"tab": pyatspi.ROLE_PAGE_TAB, "button": pyatspi.ROLE_PUSH_BUTTON}
SCREEN = (600, 940)  # the virtual output of every session, see run_linux.sh


def run(*cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


class LineHelper:
    """A helper process that takes one command per line and answers "ok" (wlpointer,
    gnome_input.py)."""

    def __init__(self, cmd):
        self.proc = subprocess.Popen(cmd, text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        if self.proc.stdout.readline().strip() != "ready":
            raise RuntimeError(f"{cmd[0]} did not start")

    def send(self, command):
        self.proc.stdin.write(command + "\n")
        self.proc.stdin.flush()
        reply = self.proc.stdout.readline().strip()
        if reply != "ok":
            raise RuntimeError(f"{command!r}: {reply or 'the helper exited'}")


# ---------------------------------------------------------------------- input, per session

class X11Input:
    def click(self, x, y):
        run("xdotool", "mousemove", str(x), str(y), "click", "1")

    def scroll(self, x, y, steps):
        button = "5" if steps > 0 else "4"
        run("xdotool", "mousemove", str(x), str(y), "click", "--repeat", str(abs(steps)), button)

    def key(self, combo, repeat=1):
        run("xdotool", "key", "--clearmodifiers", "--repeat", str(repeat), combo)

    def type(self, text):
        run("xdotool", "type", "--delay", "20", text)


class SwayInput:
    def __init__(self):
        # One virtual pointer for the whole session (see e2e/wlpointer).
        self.pointer = LineHelper([os.environ["FWG_WLPOINTER"], *map(str, SCREEN)])

    def click(self, x, y):
        self.pointer.send(f"click {x} {y}")

    def scroll(self, x, y, steps):
        self.pointer.send(f"scroll {x} {y} {steps}")

    def key(self, combo, repeat=1):
        # wtype's modifiers aren't reliable on a fresh virtual keyboard; callers avoid them.
        *mods, key = combo.split("+")
        args = []
        for m in mods:
            args += ["-M", m]
        args += (["-k", key] if len(key) > 1 else [key]) * repeat
        for m in mods:
            args += ["-m", m]
        run("wtype", *args)

    def type(self, text):
        run("wtype", "-d", "20", text)


class GnomeInput:
    def __init__(self, here):
        self.helper = LineHelper(["python3", str(here / "gnome_input.py")])

    def click(self, x, y):
        self.helper.send(f"click {x} {y}")

    def scroll(self, x, y, steps):
        self.helper.send(f"scroll {x} {y} {steps}")

    def key(self, combo, repeat=1):
        for _ in range(repeat):
            self.helper.send(f"key {combo}")

    def type(self, text):
        self.helper.send(f"type {text}")


# evdev key codes for a US keyboard, for KWin's fake input (it takes codes, not characters).
_KEYS = {"Escape": 1, "BackSpace": 14, "Tab": 15, "Return": 28, "Enter": 28, "ctrl": 29,
         "shift": 42, "alt": 56, "space": 57, "Home": 102, "Up": 103, "Left": 105, "Right": 106,
         "End": 107, "Down": 108}
_ROWS = [("1234567890-=", 2), ("qwertyuiop[]", 16), ("asdfghjkl;'", 30), ("zxcvbnm,./", 44)]
_SHIFTED = dict(zip('!@#$%^&*()_+{}:"<>?~|', "1234567890-=[];',./`\\"))
_CHARS = {c: code + i for row, code in _ROWS for i, c in enumerate(row)}
_CHARS.update({" ": 57, "`": 41, "\\": 43})


class KdeInput:
    def __init__(self):
        self.device = LineHelper([os.environ["FWG_WLPOINTER"], *map(str, SCREEN)])

    def click(self, x, y):
        self.device.send(f"click {x} {y}")

    def scroll(self, x, y, steps):
        self.device.send(f"scroll {x} {y} {steps}")

    def _tap(self, code, shift=False):
        if shift:
            self.device.send(f"key {_KEYS['shift']} 1")
        self.device.send(f"key {code} 1")
        self.device.send(f"key {code} 0")
        if shift:
            self.device.send(f"key {_KEYS['shift']} 0")

    def key(self, combo, repeat=1):
        *mods, key = combo.split("+")
        code = _KEYS.get(key) or _CHARS[key.lower()]
        for _ in range(repeat):
            for m in mods:
                self.device.send(f"key {_KEYS[m.lower()]} 1")
            self._tap(code)
            for m in reversed(mods):
                self.device.send(f"key {_KEYS[m.lower()]} 0")

    def type(self, text):
        for ch in text:
            if ch in _SHIFTED:
                self._tap(_CHARS[_SHIFTED[ch]], shift=True)
            elif ch.isupper():
                self._tap(_CHARS[ch.lower()], shift=True)
            else:
                self._tap(_CHARS[ch])
            time.sleep(0.01)


# ---------------------------------------------------------------------- backend

class Backend:
    def __init__(self, app, env, out, session):
        self.app_path, self.out, self.session = app, out, session
        self.env = dict(env)
        self.proc = None
        self.frame = None
        self.here = Path(__file__).resolve().parent
        self.portal = os.environ.get("FWG_PORTAL", "fake")
        self.portal_log = Path(os.environ["FWG_PORTAL_LOG"])
        if session in ("gnome", "kde"):
            # Wayland doesn't tell clients where their window is; maximized, it's predictable.
            self.env["FWG_E2E_MAXIMIZED"] = "1"
        self.input = {"x11": X11Input, "sway": SwayInput, "kde": KdeInput}.get(session)
        self.input = GnomeInput(self.here) if session == "gnome" else self.input()
        if session == "gnome":
            self.input.key("Escape")  # GNOME starts in the Activities overview
            time.sleep(1)

    # ------------------------------------------------------------------ lifecycle

    def start(self):
        self.proc = subprocess.Popen([self.app_path], env=self.env,
                                     stdout=open(self.out / "app.log", "a"), stderr=subprocess.STDOUT)
        deadline = time.time() + 30
        while time.time() < deadline and not self.frame:
            if self.proc.poll() is not None:
                raise RuntimeError(f"the app exited with code {self.proc.returncode}")
            self.frame = self._find_frame()
            time.sleep(0.5)
        if not self.frame:
            raise RuntimeError("no accessible window: is the AT-SPI bus enabled?")
        time.sleep(1)  # let a maximized window settle at its final size
        self.focus()
        if self.session == "sway":
            tree = json.loads(run("swaymsg", "-t", "get_tree"))
            ids = [n.get("app_id") for n in _walk(tree) if n.get("name") == TITLE]
            if APP_ID not in ids:
                raise RuntimeError(f"Wayland app_id is {ids}, expected {APP_ID}")

    def _find_frame(self):
        desktop = pyatspi.Registry.getDesktop(0)
        for app in desktop:
            if app is None:
                continue
            try:
                for child in app:
                    if child is not None and child.name == TITLE:
                        return child
            except Exception:
                continue
        return None

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.frame = None

    def restart(self):
        self.stop()
        time.sleep(1)
        self.start()

    def focus(self):
        if self.session == "x11":
            win = run("xdotool", "search", "--name", TITLE).split()[0]
            subprocess.run(["xdotool", "windowfocus", "--sync", win], check=False)

    # ------------------------------------------------------------------ elements

    def _walk(self, node, depth=0):
        if node is None or depth > 60:
            return
        yield node
        try:
            children = list(node)
        except Exception:
            return
        for child in children:
            yield from self._walk(child, depth + 1)

    def find(self, name, role=None):
        want = ROLES.get(role)
        try:
            for node in self._walk(self.frame):
                if node.name == name and (want is None or node.getRole() == want):
                    # Prefer the actionable element over its inner label of the same name.
                    if node.getRole() == pyatspi.ROLE_LABEL:
                        parent = node.parent
                        if parent is not None and parent.name == name:
                            return parent
                    return node
        except Exception:  # the tree changes while we walk it
            return None
        return None

    def activate(self, node):
        """Runs the element's (or its nearest clickable ancestor's) default action; falls back to
        a real click in the middle of the element."""
        target = node
        for _ in range(4):
            try:
                action = target.queryAction()
                names = [action.getName(i) for i in range(action.nActions)]
                if names:
                    action.doAction(names.index("click") if "click" in names else 0)
                    return
            except NotImplementedError:
                pass
            target = target.parent
            if target is None:
                break
        x, y, w, h = self._screen_rect(node)
        self.input.click(x + w // 2, y + h // 2)

    def _screen_rect(self, node):
        """The element's rectangle on screen. X11 reports screen coordinates; on Wayland they are
        relative to the window, which is either tiled full screen (sway) or maximized (GNOME,
        KDE): it then ends at the bottom of the screen, below GNOME's top bar."""
        e = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        if self.session in ("gnome", "kde"):
            frame = self.frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            return e.x - frame.x, e.y - frame.y + SCREEN[1] - frame.height, e.width, e.height
        return e.x, e.y, e.width, e.height

    def fill(self, node, text):
        self.activate(node)  # focuses the field
        time.sleep(0.3)
        self.focus()
        # Clear without modifier keys: wtype's Ctrl isn't reliable on a fresh virtual keyboard.
        self.input.key("End")
        self.input.key("BackSpace", repeat=80)
        self.input.type(text)
        time.sleep(0.3)

    # ------------------------------------------------------------------ input

    def key(self, name):
        self.focus()
        self.input.key(name)
        time.sleep(0.8)

    def scroll(self, direction):
        x, y, w, h = self._screen_rect(self.frame)
        # Three notches per step, like the Windows backend.
        self.input.scroll(max(x, 0) + w // 2, max(y, 0) + h // 2, 3 * direction)

    def clipboard(self):
        if self.session == "x11":
            cmd, env = ["xclip", "-o", "-selection", "clipboard"], None
        elif self.session == "gnome":
            # Mutter has no data-control protocol for wl-paste; its Xwayland shares the clipboard.
            cmd = ["xclip", "-o", "-selection", "clipboard"]
            env = dict(os.environ, DISPLAY=os.environ["FWG_XWAYLAND_DISPLAY"],
                       XAUTHORITY=os.environ.get("FWG_XWAYLAND_AUTH", ""))
        else:
            cmd, env = ["wl-paste", "-n"], None
        try:
            return subprocess.run(cmd, capture_output=True, text=True, timeout=5, env=env).stdout
        except Exception:
            return None

    def pick_file(self, path):
        if self.portal == "fake":
            # fake_portal.py answers the request by itself; check that the app actually asked.
            deadline = time.time() + 20
            while time.time() < deadline:
                if self.portal_log.exists() and "OpenFile" in self.portal_log.read_text():
                    return
                time.sleep(0.25)
            raise RuntimeError("the app never asked the file chooser portal for a file")
        app = dialog.pick(str(path), type_text=self.input.type)
        with open(self.portal_log, "a") as log:
            log.write(json.dumps({"call": "OpenFile", "dialog": app}) + "\n")

    def screenshot(self, path):
        if self.session == "x11":
            win = run("xdotool", "search", "--name", TITLE).split()[0]
            run("import", "-window", win, str(path))
        elif self.session == "sway":
            run("grim", str(path))
        elif self.session == "kde":
            _kwin_screenshot(path)
        else:
            raise NotImplementedError("GNOME Shell refuses screenshots from ordinary clients")

    def dump(self):
        lines = []
        for node in self._walk(self.frame):
            try:
                lines.append(f"{node.getRoleName():14} {node.name!r}")
            except Exception:
                pass
        return "\n".join(lines)


def _kwin_screenshot(path):
    """Screen capture through org.kde.KWin.ScreenShot2 (allowed because run_linux.sh starts KWin
    with KWIN_WAYLAND_NO_PERMISSION_CHECKS=1). KWin writes raw pixels into a pipe."""
    import dbus
    from PIL import Image

    read_fd, write_fd = os.pipe()
    shot = dbus.SessionBus().get_object("org.kde.KWin.ScreenShot2", "/org/kde/KWin/ScreenShot2")
    meta = shot.CaptureActiveScreen({}, dbus.types.UnixFd(write_fd),
                                    dbus_interface="org.kde.KWin.ScreenShot2")
    os.close(write_fd)
    with os.fdopen(read_fd, "rb") as pipe:
        data = pipe.read()
    width, height, stride = int(meta["width"]), int(meta["height"]), int(meta["stride"])
    # QImage::Format_ARGB32(_Premultiplied) / RGB32: little-endian BGRA.
    Image.frombuffer("RGBA", (width, height), data, "raw", "BGRA", stride, 1).save(path)


def _walk(tree):
    yield tree
    for key in ("nodes", "floating_nodes"):
        for child in tree.get(key, []):
            yield from _walk(child)
