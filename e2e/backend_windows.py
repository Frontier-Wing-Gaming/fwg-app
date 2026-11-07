"""Windows backend for driver.py: UI Automation (pywinauto) on the app's accesskit tree."""

import subprocess
import time

from PIL import ImageGrab
from pywinauto import Application, Desktop, clipboard, keyboard, mouse

ROLES = {"tab": "TabItem", "button": "Button", "text-input": "Edit"}


class Backend:
    def __init__(self, app, env, out, _session):
        self.app_path, self.env, self.out = app, env, out
        self.proc = None
        self.window = None

    def start(self):
        self.proc = subprocess.Popen([self.app_path], env=self.env,
                                     stdout=open(self.out / "app.log", "a"), stderr=subprocess.STDOUT)
        app = Application(backend="uia").connect(process=self.proc.pid, timeout=30)
        self.window = app.window(title="Frontier Gaming Network")
        self.window.wait("visible", timeout=30)
        self.window.set_focus()

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()

    def restart(self):
        # Close the window normally, as a user would, then start again.
        self.window.close()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.stop()
        self.start()

    def _elements(self):
        return self.window.descendants()

    def find(self, name, role=None):
        """An element whose accessible name is `name` (exact match), or None."""
        want = ROLES.get(role)
        try:
            for el in self._elements():
                info = el.element_info
                if info.name == name and (want is None or info.control_type == want):
                    # Prefer the actionable element over its inner label of the same name.
                    if info.control_type == "Text":
                        parent = el.parent()
                        if parent and parent.element_info.name == name:
                            return parent
                    return el
        except Exception:  # the tree changes while we walk it
            return None
        return None

    def activate(self, el):
        try:
            el.invoke()
        except Exception:
            el.click_input()

    def scroll(self, direction):
        """direction 1 = down, -1 = up, with the mouse over the middle of the page."""
        r = self.window.rectangle()
        x, y = (r.left + r.right) // 2, (r.top + r.bottom) // 2
        mouse.scroll(coords=(x, y), wheel_dist=-3 * direction)

    def fill(self, el, text):
        el.iface_value.SetValue(text)
        time.sleep(0.3)

    def key(self, name):
        self.window.set_focus()
        keyboard.send_keys({"Escape": "{ESC}"}.get(name, name))
        time.sleep(0.8)

    def clipboard(self):
        try:
            return clipboard.GetData()
        except Exception:
            return None

    def pick_file(self, path):
        """Fill in the standard Windows "Open" dialog that rfd shows."""
        dialog = Desktop(backend="uia").window(class_name="#32770", top_level_only=False)
        dialog.wait("visible", timeout=20)
        name = dialog.child_window(auto_id="1148", control_type="Edit")
        name.wait("ready", timeout=10)
        name.set_edit_text(str(path))
        time.sleep(0.3)
        name.type_keys("{ENTER}")
        dialog.wait_not("visible", timeout=10)

    def screenshot(self, path):
        r = self.window.rectangle()
        ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom), all_screens=True).save(path)

    def dump(self):
        lines = []
        for el in self._elements():
            info = el.element_info
            lines.append(f"{info.control_type:12} {info.name!r}")
        return "\n".join(lines)
