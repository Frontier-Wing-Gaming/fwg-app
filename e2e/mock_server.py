"""Mock of the website API for the desktop end-to-end tests (standard library only).

    python mock_server.py <port> <out_dir>

Serves fixed public content, a staff login (e2e@example.com / correct-horse-battery), the admin
lists the staff screen loads, and the gallery upload. Every login, logout and upload is recorded;
GET /e2e/state returns them so the test driver can assert on what the app actually sent.
Uploaded images are saved to <out_dir>/upload-<n>.<ext>.
"""

import json
import sys
import threading
import zlib
import struct
from email.parser import BytesParser
from email.policy import HTTP
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

EMAIL = "e2e@example.com"
PASSWORD = "correct-horse-battery"
TOKEN = "e2e-token-1"
USER = {"name": "E2E Tester", "email": EMAIL, "role": "owner"}

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else ".")
OUT.mkdir(parents=True, exist_ok=True)
ROOT = f"http://127.0.0.1:{PORT}"

state = {"logins": [], "logouts": 0, "uploads": [], "requests": []}
lock = threading.Lock()


def png(width, height, rgb):
    """A solid-colour PNG, so the gallery and banners have real images to decode."""
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b""))


IMAGES = {
    "/e2e/gallery-1.png": png(320, 200, (40, 160, 220)),
    "/e2e/gallery-2.png": png(320, 200, (230, 90, 40)),
    "/e2e/banner.png": png(480, 160, (60, 200, 120)),
}

GAME = {
    "name": "E2E Craft",
    "slug": "e2e-craft",
    "url": f"{ROOT}/servers/e2e-craft",
    "genre": "Sandbox",
    "accent": "#4caf50",
    "description": "A test server that only exists in the end-to-end tests.",
    "image": f"{ROOT}/e2e/banner.png",
    "server": {"type": "minecraft", "host": "e2e.example.test", "port": 25565,
               "connect": "e2e.example.test"},
    "join": ["Open the game", "Add e2e.example.test"],
}

PUBLIC = {
    "/news.php": {"items": [
        {"id": 1, "title": "E2E headline", "body": "First **test** announcement.",
         "published_at": "2026-11-07"},
        {"id": 2, "title": "Second E2E post", "body": "Another one.", "published_at": "2026-11-01"},
    ]},
    "/streams.php": {"items": [
        {"login": "e2e_streamer", "display_name": "E2E Streamer", "live": True,
         "title": "Testing live", "game": "E2E Craft"},
    ]},
    "/gallery.php": {"items": [
        {"id": 1, "title": "E2E gallery one", "image_url": f"{ROOT}/e2e/gallery-1.png"},
        {"id": 2, "title": "E2E gallery two", "image_url": f"{ROOT}/e2e/gallery-2.png"},
    ]},
    "/games.php": {"items": [GAME]},
    "/servers.php": {"checked_at": 1794000000, "summary": {"hosted": 1, "online": 1, "players": 3},
                     "items": {"e2e-craft": {"status": "online", "players": 3, "max_players": 20,
                                             "map": "1.21.1", "connect": "e2e.example.test"}}},
    "/about.php": {"name": "Frontier Gaming Network", "tagline": "E2E tagline",
                   "description": "E2E description.",
                   "about": {"intro": "E2E intro.", "body": ["E2E body."]},
                   "staff": [{"name": "E2E Tester", "role": "Owner", "group": "Leadership",
                              "bio": "Writes tests.", "initials": "ET"}]},
}

ADMIN = {
    "/admin/news.php": {"items": [{"id": 1, "title": "E2E headline", "body": "b", "published": 1,
                                   "published_at": "2026-11-07"}]},
    "/admin/gallery.php": {"items": []},
    "/admin/staff.php": {"items": [{"id": 1, "name": "E2E Tester", "email": EMAIL, "role": "owner",
                                    "active": 1, "created_at": "2026-11-01"}]},
    "/admin/push-receipts.php": {"items": []},
    "/admin/servers.php": {"items": [], "options": {}},
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print("mock:", fmt % args, flush=True)

    def reply(self, status, body, content_type="application/json"):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def api_path(self):
        path = self.path.split("?", 1)[0]
        return path[len("/api/v1"):] if path.startswith("/api/v1") else None

    def authorized(self):
        return self.headers.get("Authorization") == f"Bearer {TOKEN}"

    def body(self):
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/e2e/state":
            with lock:
                return self.reply(200, state)
        if path in IMAGES:
            return self.reply(200, IMAGES[path], "image/png")
        api = self.api_path()
        with lock:
            state["requests"].append(f"GET {api or path}")
        if api in PUBLIC:
            return self.reply(200, PUBLIC[api])
        if api in ADMIN:
            if not self.authorized():
                return self.reply(401, {"error": "Not signed in."})
            return self.reply(200, ADMIN[api])
        self.reply(404, {"error": f"No mock for {path}"})

    def do_POST(self):
        api = self.api_path()
        with lock:
            state["requests"].append(f"POST {api}")
        if api == "/auth/login.php":
            creds = json.loads(self.body() or b"{}")
            ok = creds.get("email") == EMAIL and creds.get("password") == PASSWORD
            with lock:
                state["logins"].append({"email": creds.get("email"), "ok": ok})
            if not ok:
                return self.reply(422, {"error": "Invalid email or password."})
            return self.reply(200, {"token": TOKEN, "user": USER})
        if api == "/auth/logout.php":
            with lock:
                state["logouts"] += 1
            return self.reply(200, {"success": True})
        if api == "/admin/gallery-upload.php":
            if not self.authorized():
                return self.reply(401, {"error": "Not signed in."})
            return self.upload()
        self.reply(404, {"error": f"No mock for POST {api}"})

    def upload(self):
        raw = self.body()
        message = BytesParser(policy=HTTP).parsebytes(
            b"Content-Type: " + self.headers["Content-Type"].encode() + b"\r\n\r\n" + raw)
        fields, image = {}, None
        for part in message.iter_parts():
            name = part.get_param("name", header="content-disposition")
            if part.get_filename():
                image = (part.get_filename(), part.get_content_type(), part.get_payload(decode=True))
            else:
                fields[name] = part.get_content()
        if image is None:
            return self.reply(422, {"error": "No image."})
        with lock:
            n = len(state["uploads"]) + 1
            ext = image[1].split("/")[-1]
            saved = OUT / f"upload-{n}.{ext}"
            saved.write_bytes(image[2])
            state["uploads"].append({"title": fields.get("title"), "filename": image[0],
                                     "mime": image[1], "bytes": len(image[2]), "saved": str(saved)})
        self.reply(200, {"item": {"id": str(100 + n), "title": fields.get("title", ""),
                                  "image_url": f"{ROOT}/e2e/gallery-1.png"}})


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"mock API on {ROOT}/api/v1", flush=True)
    server.serve_forever()
