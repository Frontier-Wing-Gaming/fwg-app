"""Desktop end-to-end test: drives the real app through its accessibility tree.

    python e2e/driver.py --app <path to fwg[.exe]> --out <dir> [--session x11|sway|gnome|kde]

Starts e2e/mock_server.py, launches an app built with `--features e2e` against it, then walks
through the public screens, staff sign-in, a gallery upload through the system file picker, a
restart (the session must come back from the OS keyring) and sign-out. Screenshots of every step
and the app's log land in --out. Exits non-zero on the first failed check.

The platform parts (finding elements, input, screenshots, clipboard, the file dialog) live in
backend_windows.py and backend_linux.py.
"""

import argparse
import json
import os
import subprocess
import sys
import time
import traceback
import urllib.request
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
PORT = 8765
API = f"http://127.0.0.1:{PORT}/api/v1"
EMAIL = "e2e@example.com"
PASSWORD = "correct-horse-battery"


class Failed(Exception):
    pass


def mock_state():
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/e2e/state", timeout=5) as r:
        return json.load(r)


def wait_until(check, what, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.25)
    raise Failed(f"timed out waiting for {what}")


def make_fixture(path):
    """400x200 JPEG tagged "rotate 90° clockwise" (Orientation 6) with a GPS position."""
    image = Image.new("RGB", (400, 200), (200, 40, 40))
    image.paste((40, 40, 200), (0, 0, 100, 200))  # a blue band on the left, to see the rotation
    exif = Image.Exif()
    exif[0x0112] = 6  # Orientation
    exif[0x010F] = "E2E Camera"  # Make
    # GPSLatitudeRef/GPSLatitude/GPSLongitudeRef/GPSLongitude (a dict works in Pillow 10 and later)
    exif[0x8825] = {1: "N", 2: (51.0, 30.0, 12.0), 3: "W", 4: (0.0, 7.0, 39.0)}
    image.save(path, "JPEG", exif=exif, quality=90)
    check = Image.open(path).getexif()
    assert check.get(0x0112) == 6 and check.get_ifd(0x8825), "fixture EXIF not written"


def check_upload(saved):
    """The app must apply the rotation and strip all EXIF (including GPS) before uploading."""
    image = Image.open(saved)
    if image.format != "JPEG":
        raise Failed(f"upload is {image.format}, expected JPEG")
    if image.size != (200, 400):
        raise Failed(f"upload is {image.size}, expected (200, 400): rotation not applied")
    exif = image.getexif()
    if exif.get_ifd(0x8825) or 0x8825 in exif:
        raise Failed("upload still contains GPS data")
    if len(exif):
        raise Failed(f"upload still contains EXIF tags: {dict(exif)}")
    # Orientation 6 turns the left (blue) band into the top band.
    r, g, b = image.convert("RGB").getpixel((100, 20))
    if not b > r:
        raise Failed(f"top of the upload is {(r, g, b)}, expected the blue band: rotated the wrong way")


class Run:
    def __init__(self, backend, out):
        self.ui = backend
        self.out = out
        self.step = 0

    def shot(self, name):
        self.step += 1
        try:
            self.ui.screenshot(self.out / f"{self.step:02d}-{name}.png")
        except NotImplementedError as e:
            if self.step == 1:
                print(f"    (no screenshots: {e})", flush=True)

    def expect(self, name, timeout=20, role=None):
        """Waits for an element, scrolling the page when it may be below or above the fold.
        Only what is on screen is in the accessibility tree, as for a screen-reader user."""
        deadline = time.time() + timeout
        moves = [0] * 6 + [1] * 15 + [-1] * 30  # wait in place, then scroll down, then up
        i = 0
        while time.time() < deadline:
            el = self.ui.find(name, role=role)
            if el:
                return el
            step = moves[i % len(moves)]
            if step:
                self.ui.scroll(step)
            time.sleep(0.25 if step else 0.4)
            i += 1
        raise Failed(f'timed out waiting for "{name}" on screen')

    def tap(self, name, timeout=20):
        self.ui.activate(self.expect(name, timeout))
        time.sleep(0.6)

    def public_screens(self):
        self.expect("Frontier Gaming Network")
        self.expect("3")  # players in-game, from the mock
        self.shot("home")
        for tab, text in [("News", "E2E headline"), ("Live", "E2E Streamer"),
                          ("Gallery", "E2E gallery one"), ("More", "Staff administration")]:
            self.ui.activate(self.expect(tab, role="tab"))
            self.expect(text)
            self.shot(tab.lower())

    def server_page(self):
        self.ui.activate(self.expect("Home", role="tab"))
        self.tap("Browse servers")
        self.tap("E2E Craft")
        self.expect("Server address")
        self.tap("Copy address")
        self.expect("Address copied to the clipboard.")
        clip = wait_until(lambda: "e2e.example.test" in (self.ui.clipboard() or ""), "clipboard", 5)
        self.shot("server-copied")
        self.ui.key("Escape")
        self.expect("WORLDS WE CALL HOME")  # back on the servers list
        self.ui.key("Escape")
        self.expect("Browse servers")  # back on Home
        return clip

    def open_staff(self):
        self.ui.activate(self.expect("More", role="tab"))
        self.tap("Staff administration")
        self.expect("Staff administration")

    def sign_in(self):
        self.open_staff()
        self.ui.fill(self.expect("Email"), EMAIL)
        self.ui.fill(self.expect("Password"), "wrong-password")
        self.tap("Sign in")
        self.expect("Invalid email or password.")
        self.shot("sign-in-rejected")
        self.ui.fill(self.expect("Password"), PASSWORD)
        self.tap("Sign in")
        self.expect("Sign out", timeout=30)
        logins = mock_state()["logins"]
        if [l["ok"] for l in logins] != [False, True]:
            raise Failed(f"unexpected logins: {logins}")
        self.shot("signed-in")

    def upload(self, fixture):
        self.ui.fill(self.expect("Caption"), "E2E upload")
        self.tap("Choose an image")
        self.ui.pick_file(fixture)
        self.expect("Choose a different image", timeout=30)
        self.shot("image-picked")
        self.tap("Publish to gallery")
        uploads = wait_until(lambda: mock_state()["uploads"], "the upload to reach the mock", 30)
        up = uploads[0]
        if up["title"] != "E2E upload" or up["mime"] != "image/jpeg":
            raise Failed(f"unexpected upload: {up}")
        check_upload(up["saved"])
        time.sleep(1)
        self.shot("uploaded")

    def session_restored(self):
        self.open_staff()
        self.expect("Sign out", timeout=30)
        self.shot("session-restored")

    def sign_out(self):
        self.tap("Sign out")
        self.expect("Staff sign in", timeout=30)
        wait_until(lambda: mock_state()["logouts"] == 1, "the logout request")
        self.shot("signed-out")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--session", default="x11", choices=["x11", "sway", "wayland", "gnome", "kde"])
    args = parser.parse_args()
    if args.session == "wayland":  # the original name of the sway session
        args.session = "sway"

    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    data = out / "data"
    fixture = out / "fixture.jpg"
    make_fixture(fixture)

    if sys.platform == "win32":
        from backend_windows import Backend
    else:
        from backend_linux import Backend

    mock = subprocess.Popen([sys.executable, str(HERE / "mock_server.py"), str(PORT), str(out / "mock")],
                            stdout=open(out / "mock.log", "w"), stderr=subprocess.STDOUT)
    wait_until(lambda: _alive(), "the mock server", 10)

    env = dict(os.environ, FWG_API_ROOT=API, LOCALAPPDATA=str(data), XDG_DATA_HOME=str(data))
    backend = Backend(str(Path(args.app).resolve()), env, out, args.session)
    run = Run(backend, out)
    checks = [
        ("public screens", run.public_screens),
        ("server page, clipboard and Escape", run.server_page),
        ("staff sign-in", run.sign_in),
        ("gallery upload through the file picker", lambda: run.upload(fixture)),
        ("restart keeps the session (OS keyring)", lambda: (backend.restart(), run.session_restored())),
        ("sign-out", run.sign_out),
    ]
    failed = None
    name = "start the app"
    try:
        backend.start()
        for name, check in checks:
            print(f"--- {name}", flush=True)
            check()
            print(f"    ok", flush=True)
    except Exception as e:  # report every kind of failure the same way, with a screenshot
        failed = f"{name}: {e!r}"
        traceback.print_exc()
        try:
            run.shot("FAILED")
            (out / "tree.txt").write_text(backend.dump(), encoding="utf-8")
        except Exception as shot_error:
            print(f"(no failure screenshot: {shot_error})")
    finally:
        backend.stop()
        mock.kill()
    if failed:
        print(f"FAILED: {failed}")
        print("app log:\n" + (out / "app.log").read_text(errors="replace")[-3000:])
        sys.exit(1)
    print("all end-to-end checks passed")


def _alive():
    try:
        mock_state()
        return True
    except OSError:
        return False


if __name__ == "__main__":
    main()
