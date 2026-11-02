#!/usr/bin/env python3
"""Import a checkout of ProfessorZhi/gwy-zhenti into a separate local bank."""

import argparse
import json
from pathlib import Path
import sys

import yaml

from importer import import_bank
from verify import verify_bank


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        help="existing upstream checkout (never fetched automatically)",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="separate local question-bank directory",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="preserve manual edits and known UUIDs; report conflicts",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--dry-run",
        action="store_true",
        help="parse and audit without writing anything",
    )
    mode.add_argument(
        "--verify",
        action="store_true",
        help="read-only validation of the existing bank",
    )
    args = parser.parse_args(argv)
    if not args.verify and args.source is None:
        parser.error("--source is required for import")
    try:
        if args.verify:
            report = verify_bank(args.output)
            failed = bool(report["errors"])
        else:
            report = import_bank(
                args.source, args.output, update=args.update, dry_run=args.dry_run
            )
            failed = any(
                report[key] for key in ("conflicts", "parse_failures", "image_errors")
            )
        print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
        return 1 if failed else 0
    except (OSError, ValueError, yaml.YAMLError) as error:
        print(f"question-bank: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
