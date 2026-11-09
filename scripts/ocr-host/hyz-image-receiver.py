#!/usr/bin/env python3
"""Authenticated temporary image receiver for the hyz-things Codex host.

Writes one validated JPG/PNG under <project>/tmp/hyz-ai-uploads/ and returns
its absolute path. The RK3568 portal is the only intended caller; its upload
route requires an administrator session and CSRF, and this receiver still
validates the file, size and disk quota.
"""
from __future__ import annotations

import json
import os
import socketserver
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

PROJECT_ROOT = Path(__file__).resolve().parents[2]
UPLOAD_DIR = PROJECT_ROOT / "tmp" / "hyz-ai-uploads"
MAX_IMAGE_BYTES = 8 * 1024 * 1024
MAX_IMAGE_SIDE = 12_000
MAX_IMAGE_PIXELS = 40_000_000
MAX_TOTAL_BYTES = 512 * 1024 * 1024
DEFAULT_TTL = 6 * 60 * 60
CLEANUP_INTERVAL = 10 * 60
IMAGE_TYPES = {"image/png": "png", "image/jpeg": "jpg"}


def png_dimensions(data: bytes) -> tuple[int, int] | None:
    if len(data) < 33 or data[:8] != b"\x89PNG\r\n\x1a\n":
        return None
    if int.from_bytes(data[8:12], "big") != 13 or data[12:16] != b"IHDR":
        return None
    width = int.from_bytes(data[16:20], "big")
    height = int.from_bytes(data[20:24], "big")
    if not (
        data[24] > 0
        and data[25] in (0, 2, 3, 4, 6)
        and data[26] == 0
        and data[27] == 0
        and data[28] in (0, 1)
    ):
        return None
    return width, height


def jpeg_dimensions(data: bytes) -> tuple[int, int] | None:
    if len(data) < 4 or data[0:2] != b"\xff\xd8":
        return None
    offset = 2
    sof = {0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF}
    while offset + 4 <= len(data):
        if data[offset] != 0xFF:
            offset += 1
            continue
        marker = data[offset + 1]
        if marker == 0xFF:
            offset += 1
            continue
        if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
            offset += 2
            continue
        if marker in (0xD9, 0xDA):
            return None
        length = int.from_bytes(data[offset + 2 : offset + 4], "big")
        if length < 2 or offset + 2 + length > len(data):
            return None
        if marker in sof:
            if length < 7:
                return None
            height = int.from_bytes(data[offset + 5 : offset + 7], "big")
            width = int.from_bytes(data[offset + 7 : offset + 9], "big")
            return width, height
        offset += 2 + length
    return None


def validate_image(data: bytes, media_type: str) -> tuple[int, int] | None:
    if not data or len(data) > MAX_IMAGE_BYTES:
        return None
    if media_type == "image/png":
        size = png_dimensions(data)
    elif media_type == "image/jpeg":
        size = jpeg_dimensions(data)
    else:
        return None
    if size is None:
        return None
    width, height = size
    if not (
        0 < width <= MAX_IMAGE_SIDE
        and 0 < height <= MAX_IMAGE_SIDE
        and width * height <= MAX_IMAGE_PIXELS
    ):
        return None
    return size


def env_int(name: str, default: int) -> int:
    try:
        value = int(os.environ.get(name, str(default)))
    except ValueError:
        return default
    return value if value > 0 else default


def cleanup(upload_dir: Path, ttl: int) -> None:
    now = time.time()
    if not upload_dir.is_dir():
        return
    for entry in upload_dir.iterdir():
        try:
            if entry.is_file() and now - entry.stat().st_mtime > ttl:
                entry.unlink()
        except OSError:
            continue


def total_bytes(upload_dir: Path) -> int:
    total = 0
    if not upload_dir.is_dir():
        return total
    for entry in upload_dir.iterdir():
        try:
            if entry.is_file():
                total += entry.stat().st_size
        except OSError:
            continue
    return total


class Handler(BaseHTTPRequestHandler):
    server_version = "hyz-image-receiver/1"
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt: str, *args) -> None:  # noqa: A003
        # Never log bodies; only the client and request line are useful.
        print(f"{self.address_string()} {fmt % args}", flush=True)

    def respond(self, status: int, body: dict) -> None:
        payload = json.dumps(body, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        # Rejections may leave an unread body; close instead of reusing the socket.
        self.send_header("Connection", "close")
        self.close_connection = True
        self.end_headers()
        try:
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self) -> None:  # noqa: N802
        if urlparse(self.path).path != "/health":
            self.respond(404, {"error": "not_found"})
            return
        self.respond(200, {"status": "ok"})

    def do_POST(self) -> None:  # noqa: N802
        if urlparse(self.path).path != "/v1/attachments":
            self.respond(404, {"error": "not_found"})
            return
        media_type = (self.headers.get("Content-Type") or "").split(";")[0].strip()
        extension = IMAGE_TYPES.get(media_type)
        if extension is None:
            self.respond(415, {"error": "unsupported_media_type"})
            return
        try:
            length = int(self.headers.get("Content-Length", ""))
        except ValueError:
            self.respond(411, {"error": "content_length_required"})
            return
        if length <= 0 or length > MAX_IMAGE_BYTES:
            self.respond(413, {"error": "image_too_large"})
            return
        data = self.rfile.read(length)
        if len(data) != length or validate_image(data, media_type) is None:
            self.respond(400, {"error": "invalid_image"})
            return

        upload_dir = self.server.upload_dir
        upload_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        os.chmod(upload_dir, 0o700)
        cleanup(upload_dir, self.server.ttl)
        if total_bytes(upload_dir) + len(data) > self.server.max_total:
            self.respond(507, {"error": "quota_exceeded"})
            return

        file_id = str(uuid.uuid4())
        target = upload_dir / f"{file_id}.{extension}"
        temporary = upload_dir / f".{file_id}.{extension}.part"
        try:
            descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, "wb") as handle:
                handle.write(data)
            os.replace(temporary, target)
        except OSError:
            try:
                temporary.unlink()
            except OSError:
                pass
            self.respond(500, {"error": "storage_unavailable"})
            return
        os.chmod(target, 0o600)
        self.respond(
            200,
            {
                "id": file_id,
                "path": str(target),
                "expires_at": int(time.time()) + self.server.ttl,
            },
        )


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def server_bind(self) -> None:
        # Skip HTTPServer's reverse-DNS lookup, which can stall startup.
        socketserver.TCPServer.server_bind(self)
        self.server_name = self.server_address[0]
        self.server_port = self.server_address[1]

    def __init__(self, address: tuple[str, int], ttl: int, max_total: int):
        super().__init__(address, Handler)
        self.ttl = ttl
        self.max_total = max_total
        self.upload_dir = UPLOAD_DIR


def cleanup_loop(server: Server) -> None:
    while True:
        time.sleep(CLEANUP_INTERVAL)
        cleanup(server.upload_dir, server.ttl)


def main() -> None:
    bind = os.environ.get("HYZ_IMAGE_RECEIVER_BIND", "127.0.0.1:8099")
    host, _, port = bind.rpartition(":")
    if not host or not port.isdigit():
        raise SystemExit("HYZ_IMAGE_RECEIVER_BIND must be HOST:PORT")
    server = Server(
        (host, int(port)),
        env_int("HYZ_IMAGE_RECEIVER_TTL", DEFAULT_TTL),
        env_int("HYZ_IMAGE_RECEIVER_MAX_TOTAL", MAX_TOTAL_BYTES),
    )
    UPLOAD_DIR.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(UPLOAD_DIR, 0o700)
    threading.Thread(target=cleanup_loop, args=(server,), daemon=True).start()
    print(f"hyz image receiver listening on {host}:{port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
