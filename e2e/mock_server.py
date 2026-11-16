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
from urllib.parse import parse_qs

EMAIL = "e2e@example.com"
PASSWORD = "correct-horse-battery"
TOKEN = "e2e-token-1"
USER = {"name": "E2E Tester", "email": EMAIL, "role": "owner"}

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else ".")
OUT.mkdir(parents=True, exist_ok=True)
ROOT = f"http://127.0.0.1:{PORT}"

state = {"logins": [], "logouts": 0, "uploads": [], "requests": [], "rs_signups": []}
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
    "/runescape/feed.php": {"items": [
        {"id": 1, "member_id": 7, "name": "E2E Wingman", "game": "rs3", "kind": "milestone",
         "text": "reached 99 Mining", "detail": None, "happened_at": "2026-11-15T06:17:00Z"}]},
    "/runescape/competitions.php": {"items": [
        {"id": 3, "title": "E2E Skill of the Week", "game": "rs3", "game_label": "RuneScape 3",
         "metric": "skill:Mining", "metric_label": "Mining", "unit": "xp",
         "starts_at": "2026-11-15T18:00:00Z", "ends_at": "2026-11-22T18:00:00Z", "status": "running"}]},
}

RS_METRICS = [{"value": "overall", "label": "Overall", "group": "Overall"},
              {"value": "skill:Mining", "label": "Mining", "group": "Skills"},
              {"value": "activity:Zulrah", "label": "Zulrah", "group": "Bosses and activities"}]


def runescape_board(query):
    """The same members for every game and period; Overall or the Mining board."""
    metric = (query.get("metric") or ["overall"])[0]
    mining = metric == "skill:Mining"
    return {"game": (query.get("game") or ["rs3"])[0], "period": (query.get("period") or ["week"])[0],
            "metric": metric, "metric_label": "Mining" if mining else "Overall", "unit": "xp",
            "metrics": RS_METRICS, "updated_at": "2026-11-16T06:17:00Z",
            "items": [{"id": 7, "name": "E2E Wingman", "total_level": 2898, "total_xp": 412345678,
                       "rank": 1234, "value": 13034431 if mining else 412345678,
                       "level": 99 if mining else 2898, "unit": "xp",
                       "updated_at": "2026-11-16T06:17:00Z",
                       "gained": 900000 if mining else 1500000, "since": "2026-11-09T06:17:00Z",
                       "partial": False, "top_skill": None if mining else {"name": "Mining", "xp": 900000},
                       "account_type": "ironman", "account_label": "Ironman", "account_short": "IM"}]}


RS_PLAYER = {"id": 7, "name": "E2E Wingman", "games": [
    {"game": "rs3", "label": "RuneScape 3", "name": "E2E Wingman", "account_type": "ironman",
     "account_label": "Ironman", "account_short": "IM", "death": None,
     "updated_at": "2026-11-16T06:17:00Z", "total_level": 2898, "total_xp": 412345678, "rank": 1234,
     "week_gained": 1500000, "skills": [{"name": "Mining", "level": 99, "xp": 13034431, "rank": 50,
                                          "week_gained": 900000}],
     "activities": [{"name": "Zulrah", "score": 42, "rank": 9000, "week_gained": 0}],
     "quests": {"qp": 300, "qp_max": 476, "completed": 200, "started": 3, "total": 366, "quests": []}}],
    "events": [{"id": 1, "member_id": 7, "name": "E2E Wingman", "game": "rs3", "kind": "milestone",
                "text": "reached 99 Mining", "detail": None, "happened_at": "2026-11-15T06:17:00Z"}]}

RS_COMPETITION = dict(PUBLIC["/runescape/competitions.php"]["items"][0], items=[
    {"id": 7, "name": "E2E Wingman", "gained": 250000, "start": 12784431, "end": 13034431, "level": 99,
     "account_short": "IM"},
    {"id": 8, "name": "E2E Latecomer", "gained": None, "start": None, "end": None, "level": None,
     "account_short": ""}])

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
        query = parse_qs(self.path.split("?", 1)[1]) if "?" in self.path else {}
        if api == "/runescape.php":
            return self.reply(200, runescape_board(query))
        if api == "/runescape/player.php":
            if query.get("id") != ["7"]:
                return self.reply(404, {"error": "Player not found."})
            return self.reply(200, RS_PLAYER)
        if api == "/runescape/competitions.php" and "id" in query:
            if query.get("id") != ["3"]:
                return self.reply(404, {"error": "Competition not found."})
            return self.reply(200, RS_COMPETITION)
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
        if api == "/runescape/signup.php":
            signup = json.loads(self.body() or b"{}")
            with lock:
                state["rs_signups"].append(signup)
            return self.reply(201, {"success": True, "message": "Thanks! Staff will review your sign-up.",
                                    "warnings": []})
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
