#!/usr/bin/env bash
set -Eeuo pipefail

readonly SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PACKAGE_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly WORKSPACE_DIR="$(CDPATH= cd -- "${PACKAGE_DIR}/../../.." && pwd)"
readonly FRONTEND_DIR="${PACKAGE_DIR}/frontend"
readonly EXTERNALIZER="${SCRIPT_DIR}/externalize-trunk-bootstrap.py"
readonly OUTPUT_DIR="${WORKSPACE_DIR}/target/frontend-bundle"
readonly OUTPUT_FILE="${OUTPUT_DIR}/hyz-things-frontend.tar"

for command_name in node npm python3 rustup trunk tar mktemp; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'error: required command not found: %s\n' "${command_name}" >&2
        exit 127
    fi
done
readonly NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
if [[ ! "${NODE_MAJOR}" =~ ^[0-9]+$ || "${NODE_MAJOR}" -lt 20 ]]; then
    printf 'error: Node.js 20 or newer is required (found %s)\n' "$(node --version)" >&2
    exit 1
fi
if ! tar --version 2>/dev/null | grep -q 'GNU tar'; then
    printf 'error: GNU tar is required for deterministic archive metadata\n' >&2
    exit 1
fi
if ! rustup target list --installed | grep -qx 'wasm32-unknown-unknown'; then
    printf 'error: Rust target wasm32-unknown-unknown is not installed\n' >&2
    printf 'hint: run: rustup target add wasm32-unknown-unknown\n' >&2
    exit 1
fi
if [[ ! -f "${PACKAGE_DIR}/Cargo.toml" || ! -f "${PACKAGE_DIR}/package-lock.json" || ! -f "${FRONTEND_DIR}/index.html" || ! -f "${FRONTEND_DIR}/app.css" || ! -f "${EXTERNALIZER}" ]]; then
    printf 'error: incomplete things package/frontend source tree\n' >&2
    exit 1
fi
if [[ ! -x "${PACKAGE_DIR}/node_modules/.bin/tailwindcss" ]]; then
    printf 'error: locked frontend dependencies are not installed\n' >&2
    printf 'hint: run: (cd %s && npm ci)\n' "${PACKAGE_DIR}" >&2
    exit 1
fi

mkdir -p -- "${OUTPUT_DIR}"
readonly TEMP_DIR="$(mktemp -d "${OUTPUT_DIR}/.hyz-things-frontend.XXXXXX")"
cleanup() { rm -rf -- "${TEMP_DIR}"; }
trap cleanup EXIT INT TERM HUP
readonly DIST_DIR="${TEMP_DIR}/dist"
(
    cd -- "${PACKAGE_DIR}"
    NO_COLOR=true npm run build:css
    NO_COLOR=true trunk build --locked --release --dist "${DIST_DIR}" frontend/index.html
)
python3 "${EXTERNALIZER}" "${DIST_DIR}"
if [[ ! -s "${DIST_DIR}/index.html" || ! -s "${DIST_DIR}/router-bootstrap.js" ]]; then
    printf 'error: Trunk completed without producing CSP-compatible frontend assets\n' >&2
    exit 1
fi
if ! find "${DIST_DIR}" -maxdepth 1 -type f -name '*.css' -size +0c -print -quit | grep -q .; then
    printf 'error: Trunk completed without producing the Tailwind stylesheet\n' >&2
    exit 1
fi
if grep -Eiq '<style([[:space:]>])|<script[^>]*>[^<]' "${DIST_DIR}/index.html"; then
    printf 'error: frontend distribution contains inline script or style content\n' >&2
    exit 1
fi
if grep -RIEq "@import[[:space:]]+url\\(['\"]?https?://|url\\(['\"]?https?://" "${DIST_DIR}" --include='*.css'; then
    printf 'error: frontend stylesheet contains an external runtime reference\n' >&2
    exit 1
fi

readonly TEMP_ARCHIVE="${TEMP_DIR}/hyz-things-frontend.tar"
find "${DIST_DIR}" -type d -exec chmod 0755 {} +
find "${DIST_DIR}" -type f -exec chmod 0644 {} +
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner -cf "${TEMP_ARCHIVE}" -C "${DIST_DIR}" .
mv -f -- "${TEMP_ARCHIVE}" "${OUTPUT_FILE}"
printf 'frontend bundle: %s\n' "${OUTPUT_FILE}"
