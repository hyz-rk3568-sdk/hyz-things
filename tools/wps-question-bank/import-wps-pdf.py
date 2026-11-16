#!/usr/bin/env python3
"""Convert a WPS 文字 question-book PDF into hyz question-bank Markdown."""

from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

from wpsbank.cli import main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(main())
