#!/usr/bin/env python3
"""Call the local PaddleX OCR serving endpoint for one temporary attachment.

Only files inside <project>/tmp/hyz-ai-uploads/ are accepted. The endpoint is
fixed to the loopback PaddleOCR service; this script never installs, starts or
exposes anything.
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_URL = "http://127.0.0.1:8080/ocr"
MAX_BYTES = 8 * 1024 * 1024
TIMEOUT = 120


def upload_root() -> Path:
    return Path(__file__).resolve().parents[4] / "tmp" / "hyz-ai-uploads"


def checked_path(value: str) -> Path:
    path = Path(value).resolve()
    try:
        path.relative_to(upload_root().resolve())
    except ValueError:
        raise SystemExit("只允许读取本次上传的 tmp/hyz-ai-uploads/ 图片") from None
    if not path.is_file() or path.stat().st_size > MAX_BYTES:
        raise SystemExit("图片不存在或超过 8 MiB")
    return path


def collect_text(value, output: list[str]) -> None:
    if isinstance(value, dict):
        for key, item in value.items():
            if key in {"text", "rec_text", "recText", "content"} and isinstance(item, str):
                item = item.strip()
                if item and item not in output:
                    output.append(item)
            elif key == "rec_texts" and isinstance(item, list):
                for line in item:
                    if isinstance(line, str):
                        line = line.strip()
                        if line and line not in output:
                            output.append(line)
            else:
                collect_text(item, output)
    elif isinstance(value, list):
        for item in value:
            collect_text(item, output)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image")
    parser.add_argument("--url", default=os.environ.get("HYZ_OCR_URL", DEFAULT_URL))
    args = parser.parse_args()
    path = checked_path(args.image)
    payload = json.dumps(
        {
            "file": base64.b64encode(path.read_bytes()).decode(),
            "fileType": 1,
            "visualize": False,
        }
    ).encode()
    request = urllib.request.Request(
        args.url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            result = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
        print(json.dumps({"ok": False, "error": f"OCR 服务不可用：{error}"}, ensure_ascii=False))
        return 2
    if result.get("errorCode", 0) != 0:
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": f"OCR 服务返回错误：{result.get('errorMsg', result['errorCode'])}",
                },
                ensure_ascii=False,
            )
        )
        return 3
    text: list[str] = []
    collect_text(result, text)
    print(json.dumps({"ok": True, "lines": text, "raw": result}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
