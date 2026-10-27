#!/usr/bin/env python3
"""Validate frontend cache inputs and the exact archive those inputs produced."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

WORKSPACE = Path(__file__).resolve().parents[4]
PACKAGE = Path("apps/rust/things")
ARCHIVE = WORKSPACE / "target/frontend-bundle/hyz-things-frontend.tar"
METADATA = ARCHIVE.with_suffix(".json")
VERSION = 1


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fingerprint(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def walk_error(error: OSError) -> None:
    raise error


def source_files(relative: Path):
    for current, directories, names in os.walk(WORKSPACE / relative, onerror=walk_error):
        directory = Path(current)
        directories[:] = sorted(name for name in directories if name not in {
            "target", "node_modules", "playwright-report", "test-results", "__pycache__", ".git",
        })
        for name in directories:
            if (directory / name).is_symlink():
                raise ValueError(f"unsupported frontend input directory symlink: {directory / name}")
        for name in names:
            path = directory / name
            if path != WORKSPACE / PACKAGE / "frontend/router.css":
                yield path.relative_to(WORKSPACE)


def input_paths() -> list[Path]:
    required = [
        PACKAGE / name for name in (
            "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "Trunk.toml", "build.rs",
            "tools/build-frontend-bundle.sh", "tools/externalize-trunk-bootstrap.py",
            "tools/frontend-bundle-cache.py",
        )
    ] + [Path("apps/rust/contract") / name for name in ("Cargo.toml", "Cargo.lock")]
    files = set(required)
    for relative in required:
        if not (WORKSPACE / relative).is_file():
            raise ValueError(f"missing frontend input: {relative}")
    for relative in (PACKAGE / "src", PACKAGE / "frontend", Path("apps/rust/contract/src")):
        if not (WORKSPACE / relative).is_dir():
            raise ValueError(f"missing frontend input directory: {relative}")
    for relative in (PACKAGE, Path("apps/rust/contract/src")):
        files.update(source_files(relative))
    for directory in (Path("."), Path("apps"), Path("apps/rust"), PACKAGE):
        for name in (".cargo/config", ".cargo/config.toml", "rust-toolchain", "rust-toolchain.toml", ".gitignore"):
            relative = directory / name
            if (WORKSPACE / relative).is_file():
                files.add(relative)
    return sorted(files)


def input_fingerprint() -> str:
    entries = [(path.as_posix(), sha256(WORKSPACE / path)) for path in input_paths()]
    return fingerprint(entries)


def toolchain_fingerprint() -> str:
    versions = {}
    for command in (["rustc", "-vV"], ["node", "--version"], ["npm", "--version"], ["trunk", "--version"]):
        result = subprocess.run(command, cwd=WORKSPACE / PACKAGE, check=True,
                                capture_output=True, text=True, timeout=10)
        versions[command[0]] = result.stdout.strip()
    environment = {name: value for name, value in os.environ.items() if name in (
        "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "RUSTUP_TOOLCHAIN",
        "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS", "SOURCE_DATE_EPOCH",
    ) or name.startswith(("CARGO_PROFILE_RELEASE_", "TRUNK_BUILD_"))}
    cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    if not cargo_home.is_absolute():
        cargo_home = WORKSPACE / PACKAGE / cargo_home
    configs = [(name, sha256(cargo_home / name)) for name in ("config", "config.toml")
               if (cargo_home / name).is_file()]
    return fingerprint([versions, environment, configs])


def check(expected: str, toolchain: str | None = None) -> None:
    metadata = json.loads(METADATA.read_text())
    if (not isinstance(metadata, dict) or type(metadata.get("version")) is not int
            or metadata["version"] != VERSION):
        raise ValueError("unrecognized frontend cache metadata")
    if not all(isinstance(metadata.get(key), str) and len(metadata[key]) == 64
               and all(char in "0123456789abcdef" for char in metadata[key])
               for key in ("inputs_sha256", "toolchain_sha256", "archive_sha256")):
        raise ValueError("invalid frontend cache hashes")
    if toolchain is not None and metadata["toolchain_sha256"] != toolchain:
        raise ValueError("frontend build toolchain changed")
    if metadata.get("inputs_sha256") != expected:
        raise ValueError("frontend inputs changed")
    if not ARCHIVE.is_file() or ARCHIVE.stat().st_size == 0:
        raise ValueError("frontend archive is missing or empty")
    if metadata.get("archive_sha256") != sha256(ARCHIVE):
        raise ValueError("frontend archive checksum mismatch")


def record(expected: str, toolchain: str) -> None:
    if input_fingerprint() != expected:
        raise ValueError("frontend inputs changed during the build")
    if toolchain_fingerprint() != toolchain:
        raise ValueError("frontend build toolchain changed during the build")
    metadata = {"version": VERSION, "inputs_sha256": expected,
                "toolchain_sha256": toolchain, "archive_sha256": sha256(ARCHIVE)}
    with tempfile.NamedTemporaryFile(mode="w", dir=METADATA.parent, delete=False) as output:
        temporary = Path(output.name)
        try:
            json.dump(metadata, output, sort_keys=True)
            output.write("\n")
            output.close()
            os.replace(temporary, METADATA)
        finally:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("fingerprint", "toolchain", "check", "record"))
    parser.add_argument("expected", nargs="*")
    args = parser.parse_args()
    allowed = {"fingerprint": (0,), "toolchain": (0,), "check": (0, 1, 2), "record": (2,)}
    if len(args.expected) not in allowed[args.command]:
        parser.error("unexpected number of fingerprint arguments")
    try:
        if args.command == "fingerprint":
            print(input_fingerprint())
        elif args.command == "toolchain":
            print(toolchain_fingerprint())
        elif args.command == "check":
            expected = args.expected[0] if args.expected else input_fingerprint()
            check(expected, args.expected[1] if len(args.expected) == 2 else None)
        else:
            record(*args.expected)
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"frontend bundle cache: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
