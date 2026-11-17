"""Parallel page OCR through local PaddleX/PaddleOCR endpoints."""

from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import tempfile
import threading
import time
import urllib.error
import urllib.request

import pymupdf

DEFAULT_URL = "http://127.0.0.1:8080/ocr"
# Bump when request options or the block schema change, so old caches are ignored.
CACHE_VERSION = 5


class OcrError(RuntimeError):
    pass


class OcrPage:
    def __init__(
        self,
        page: int,
        dpi: int,
        width: int,
        height: int,
        blocks: list[dict],
        blanks: list[list[float]] | None = None,
    ):
        self.page = page
        self.dpi = dpi
        self.width = width
        self.height = height
        self.blocks = blocks
        self.blanks = list(blanks or [])

    def to_json(self) -> dict:
        return {
            "page": self.page,
            "dpi": self.dpi,
            "width": self.width,
            "height": self.height,
            "blocks": self.blocks,
            "blanks": self.blanks,
        }

    @classmethod
    def from_json(cls, value: dict) -> "OcrPage":
        return cls(
            int(value["page"]),
            int(value["dpi"]),
            int(value["width"]),
            int(value["height"]),
            list(value["blocks"]),
            list(value.get("blanks") or []),
        )


def _pdf_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _box_from_polygon(polygon) -> list[float] | None:
    if not isinstance(polygon, (list, tuple)) or not polygon:
        return None
    points = []
    for point in polygon:
        if isinstance(point, (list, tuple)) and len(point) == 2:
            try:
                points.append((float(point[0]), float(point[1])))
            except (TypeError, ValueError):
                return None
    if not points:
        return None
    xs = [point[0] for point in points]
    ys = [point[1] for point in points]
    return [min(xs), min(ys), max(xs), max(ys)]


def normalize_blocks(result: dict, page: int) -> list[dict]:
    root = result.get("result", result)
    if isinstance(root, list):
        root = {"ocrResults": root}
    if not isinstance(root, dict):
        raise OcrError("OCR response has no result object")
    pages = root.get("ocrResults")
    if not isinstance(pages, list) or not pages:
        raise OcrError("OCR response has no ocrResults")
    pruned = pages[0].get("prunedResult") if isinstance(pages[0], dict) else None
    if not isinstance(pruned, dict):
        raise OcrError("OCR response has no prunedResult")

    texts = pruned.get("rec_texts") or []
    scores = pruned.get("rec_scores") or []
    boxes = pruned.get("rec_boxes")
    polygons = pruned.get("rec_polys") or pruned.get("dt_polys")
    blocks = []
    for index, text in enumerate(texts):
        if not isinstance(text, str) or not text.strip():
            continue
        box = None
        if isinstance(boxes, list) and index < len(boxes):
            raw = boxes[index]
            if isinstance(raw, (list, tuple)) and len(raw) == 4:
                try:
                    box = [float(value) for value in raw]
                except (TypeError, ValueError):
                    box = None
        if box is None and isinstance(polygons, list) and index < len(polygons):
            box = _box_from_polygon(polygons[index])
        if box is None:
            continue
        try:
            score = float(scores[index]) if index < len(scores) else 0.0
        except (TypeError, ValueError):
            score = 0.0
        blocks.append(
            {
                "id": index,
                "page": page,
                "text": text.strip(),
                "confidence": score,
                "box": box,
            }
        )
    return blocks


class OcrPool:
    """One worker per endpoint; pages are rendered lazily and results cached."""

    def __init__(
        self,
        urls: list[str],
        cache_dir: Path | str,
        dpi: int = 220,
        timeout: float = 300,
        retries: int = 1,
    ):
        if not urls:
            raise ValueError("at least one OCR endpoint is required")
        self.urls = list(urls)
        self.cache_dir = Path(cache_dir)
        self.dpi = dpi
        self.timeout = timeout
        self.retries = retries

    def _cache_file(self, digest: str, page: int) -> Path:
        return self.cache_dir / digest / f"page-{page:04d}-{self.dpi}-v{CACHE_VERSION}.json"

    def _load(self, digest: str, page: int) -> OcrPage | None:
        path = self._cache_file(digest, page)
        if not path.is_file():
            return None
        try:
            return OcrPage.from_json(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, ValueError, KeyError):
            return None

    def _store(self, digest: str, value: OcrPage) -> None:
        path = self._cache_file(digest, value.page)
        path.parent.mkdir(parents=True, exist_ok=True)
        handle, temporary = tempfile.mkstemp(dir=path.parent, suffix=".json")
        try:
            with os.fdopen(handle, "w", encoding="utf-8") as output:
                json.dump(value.to_json(), output, ensure_ascii=False)
            os.replace(temporary, path)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)

    def _call(self, url: str, png: bytes, page: int) -> list[dict]:
        payload = json.dumps(
            {
                "file": base64.b64encode(png).decode(),
                "fileType": 1,
                "visualize": False,
                # Unwarping rewrites the page geometry; its boxes no longer match
                # the rendered pixels, which breaks rule/blank detection.
                "useDocOrientationClassify": False,
                "useDocUnwarping": False,
                # Short option rows are sometimes misread as rotated lines.
                "useTextlineOrientation": False,
            }
        ).encode()
        last_error = None
        for _ in range(self.retries + 1):
            request = urllib.request.Request(
                url,
                data=payload,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    result = json.loads(response.read().decode("utf-8"))
                if result.get("errorCode", 0) != 0:
                    raise OcrError(
                        f"OCR error {result.get('errorCode')}: {result.get('errorMsg')}"
                    )
                return normalize_blocks(result, page)
            except (urllib.error.URLError, TimeoutError, ValueError, OcrError) as error:
                last_error = error
                time.sleep(1)
        raise OcrError(f"OCR failed on page {page}: {last_error}")

    def recognize(
        self,
        pdf_path: Path | str,
        pages: list[int],
        progress=None,
        prepare=None,
    ) -> dict[int, OcrPage]:
        pdf_path = Path(pdf_path)
        digest = _pdf_digest(pdf_path)
        results: dict[int, OcrPage] = {}
        pending = []
        for page in pages:
            cached = self._load(digest, page)
            if cached is not None:
                results[page] = cached
            else:
                pending.append(page)
        if progress:
            progress(f"OCR cache: {len(results)} hit, {len(pending)} to run")

        if pending:
            document = pymupdf.open(str(pdf_path))
            work: queue.Queue = queue.Queue(maxsize=max(2, len(self.urls) * 2))
            lock = threading.Lock()
            errors: list[BaseException] = []

            def worker(url: str) -> None:
                while True:
                    item = work.get()
                    if item is None:
                        work.task_done()
                        return
                    page, png, width, height, blanks = item
                    try:
                        blocks = self._call(url, png, page)
                        value = OcrPage(page, self.dpi, width, height, blocks, blanks)
                        self._store(digest, value)
                        with lock:
                            results[page] = value
                        if progress:
                            progress(f"page {page}: {len(blocks)} blocks")
                    except BaseException as error:  # keep the queue moving
                        with lock:
                            errors.append(error)
                    finally:
                        work.task_done()

            threads = [
                threading.Thread(target=worker, args=(url,), daemon=True)
                for url in self.urls
            ]
            for thread in threads:
                thread.start()
            try:
                for page in pending:
                    pix = document[page - 1].get_pixmap(dpi=self.dpi)
                    png = pix.tobytes("png")
                    blanks = []
                    if prepare is not None:
                        png, blanks = prepare(document[page - 1], pix, png)
                    work.put((page, png, pix.width, pix.height, blanks))
            finally:
                for _ in threads:
                    work.put(None)
                work.join()
                document.close()
            if errors:
                raise OcrError(str(errors[0]))
        return dict(sorted(results.items()))
