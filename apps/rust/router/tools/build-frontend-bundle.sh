#!/usr/bin/env bash
set -Eeuo pipefail

readonly SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PACKAGE_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly WORKSPACE_DIR="$(CDPATH= cd -- "${PACKAGE_DIR}/../../.." && pwd)"
readonly FRONTEND_DIR="${PACKAGE_DIR}/frontend"
readonly EXTERNALIZER="${SCRIPT_DIR}/externalize-trunk-bootstrap.py"
readonly OUTPUT_DIR="${WORKSPACE_DIR}/target/frontend-bundle"
readonly OUTPUT_FILE="${OUTPUT_DIR}/router-frontend.tar"

for command_name in python3 rustup trunk tar mktemp; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'error: required command not found: %s\n' "${command_name}" >&2
        exit 127
    fi
done
if ! tar --version 2>/dev/null | grep -q 'GNU tar'; then
    printf 'error: GNU tar is required for deterministic archive metadata\n' >&2
    exit 1
fi
if ! rustup target list --installed | grep -qx 'wasm32-unknown-unknown'; then
    printf 'error: Rust target wasm32-unknown-unknown is not installed\n' >&2
    printf 'hint: run: rustup target add wasm32-unknown-unknown\n' >&2
    exit 1
fi
if [[ ! -f "${PACKAGE_DIR}/Cargo.toml" || ! -f "${FRONTEND_DIR}/index.html" || ! -f "${EXTERNALIZER}" ]]; then
    printf 'error: incomplete router package/frontend source tree\n' >&2
    exit 1
fi

mkdir -p -- "${OUTPUT_DIR}"
readonly TEMP_DIR="$(mktemp -d "${OUTPUT_DIR}/.router-frontend.XXXXXX")"
cleanup() { rm -rf -- "${TEMP_DIR}"; }
trap cleanup EXIT INT TERM HUP
readonly DIST_DIR="${TEMP_DIR}/dist"
(
    cd -- "${PACKAGE_DIR}"
    NO_COLOR=true trunk build --locked --release --dist "${DIST_DIR}" frontend/index.html
)
python3 "${EXTERNALIZER}" "${DIST_DIR}"
if [[ ! -s "${DIST_DIR}/index.html" || ! -s "${DIST_DIR}/router-bootstrap.js" ]]; then
    printf 'error: Trunk completed without producing CSP-compatible frontend assets\n' >&2
    exit 1
fi

readonly TEMP_ARCHIVE="${TEMP_DIR}/router-frontend.tar"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner -cf "${TEMP_ARCHIVE}" -C "${DIST_DIR}" .
mv -f -- "${TEMP_ARCHIVE}" "${OUTPUT_FILE}"
printf 'frontend bundle: %s\n' "${OUTPUT_FILE}"
