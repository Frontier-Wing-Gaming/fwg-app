"""Linux backend for driver.py: AT-SPI (pyatspi) on the app's accesskit tree, X11 or Wayland.

Input goes through accessibility actions where possible; keys and text go through xdotool (X11)
or wtype (Wayland), scrolling through xdotool or sway's seat cursor. Screenshots: ImageMagick
`import` (X11) or grim (Wayland). The file picker is answered by fake_portal.py.
"""

import json
import os
import subprocess
import time
from pathlib import Path

import pyatspi

TITLE = "Frontier Gaming Network"
ROLES = {"tab": pyatspi.ROLE_PAGE_TAB, "button": pyatspi.ROLE_PUSH_BUTTON}


def run(*cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


class Backend:
    def __init__(self, app, env, out, session):
        self.app_path, self.env, self.out, self.session = app, env, out, session
        self.proc = None
        self.frame = None
        self.portal_log = Path(os.environ["FWG_PORTAL_LOG"])
        self.pointer = None
        if session == "wayland":
            # One virtual pointer for the whole session (see e2e/wlpointer).
            self.pointer = subprocess.Popen([os.environ["FWG_WLPOINTER"], "600", "940"], text=True,
                                            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
            if self.pointer.stdout.readline().strip() != "ready":
                raise RuntimeError("wlpointer did not start")

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
        self.focus()
        if self.session == "wayland":
            tree = json.loads(run("swaymsg", "-t", "get_tree"))
            ids = [n.get("app_id") for n in _walk(tree) if n.get("name") == TITLE]
            if "network.frontierwinggaming.app" not in ids:
                raise RuntimeError(f"Wayland app_id is {ids}, expected network.frontierwinggaming.app")

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
        e = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        self.click(e.x + e.width // 2, e.y + e.height // 2)

    def click(self, x, y):
        if self.session == "x11":
            run("xdotool", "mousemove", str(x), str(y), "click", "1")
        else:
            self._pointer(f"click {x} {y}")

    def _pointer(self, command):
        self.pointer.stdin.write(command + "\n")
        self.pointer.stdin.flush()
        reply = self.pointer.stdout.readline().strip()
        if reply != "ok":
            raise RuntimeError(f"wlpointer: {reply or 'exited'}")

    def fill(self, node, text):
        self.activate(node)  # focuses the field
        time.sleep(0.3)
        self.focus()
        # Clear without modifier keys: wtype's Ctrl isn't reliable on a fresh virtual keyboard.
        if self.session == "x11":
            run("xdotool", "key", "End", "key", "--repeat", "80", "BackSpace")
        else:
            run("wtype", "-k", "End", *(["-k", "BackSpace"] * 80))
        self._type(text)
        time.sleep(0.3)

    # ------------------------------------------------------------------ input

    def _keys(self, combo):
        if self.session == "x11":
            run("xdotool", "key", "--clearmodifiers", combo)
        else:
            *mods, key = combo.split("+")
            args = []
            for m in mods:
                args += ["-M", m]
            args += ["-k", key] if len(key) > 1 else [key]
            for m in mods:
                args += ["-m", m]
            run("wtype", *args)

    def _type(self, text):
        if self.session == "x11":
            run("xdotool", "type", "--delay", "20", text)
        else:
            run("wtype", "-d", "20", text)

    def key(self, name):
        self.focus()
        self._keys(name)
        time.sleep(0.8)

    def scroll(self, direction):
        x, y, w, h = self._extents()
        cx, cy = x + w // 2, y + h // 2
        button = "5" if direction > 0 else "4"
        # Three notches per step, like the Windows backend.
        if self.session == "x11":
            run("xdotool", "mousemove", str(cx), str(cy), "click", "--repeat", "3", button)
        else:
            self._pointer(f"scroll {cx} {cy} {3 * direction}")

    def _extents(self):
        e = self.frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        return max(e.x, 0), max(e.y, 0), e.width, e.height

    def clipboard(self):
        cmd = ["xclip", "-o", "-selection", "clipboard"] if self.session == "x11" else ["wl-paste", "-n"]
        try:
            return subprocess.run(cmd, capture_output=True, text=True, timeout=5).stdout
        except Exception:
            return None

    def pick_file(self, path):
        """fake_portal.py answers the request by itself; check that the app actually asked."""
        deadline = time.time() + 20
        while time.time() < deadline:
            if self.portal_log.exists() and "OpenFile" in self.portal_log.read_text():
                return
            time.sleep(0.25)
        raise RuntimeError("the app never asked the file chooser portal for a file")

    def screenshot(self, path):
        if self.session == "x11":
            win = run("xdotool", "search", "--name", TITLE).split()[0]
            run("import", "-window", win, str(path))
        else:
            run("grim", str(path))

    def dump(self):
        lines = []
        for node in self._walk(self.frame):
            try:
                lines.append(f"{node.getRoleName():14} {node.name!r}")
            except Exception:
                pass
        return "\n".join(lines)


def _walk(tree):
    yield tree
    for key in ("nodes", "floating_nodes"):
        for child in tree.get(key, []):
            yield from _walk(child)
